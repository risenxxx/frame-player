// Packs Skathi's two JavaScript packages, @skathi-native/cli and @skathi-native/runtime, from the
// very checkout cargo makes of the commit src-skathi/Cargo.toml pins, so the Rust crate and the
// packages cannot come from different commits. The tarballs go where src-skathi/app/package.json
// expects them (src-skathi/app/.skathi/*.tgz, not in git).
//
// Why not npm: the build needs <sk-layer>, which no npm release has yet, and neither bun nor npm
// installs a package from a subdirectory of a git repository. When a release carries what the pin
// carries, package.json takes registry versions and this script goes.
//
// The tarballs hold the commit's files only, packed byte for byte the same on every machine, because
// bun.lock records their hashes. The CLI also needs its CSS checker, `check/sk-css.wasm.br`, which
// Skathi builds rather than commits: sk-css's `css-check` example for wasm32-unknown-unknown,
// brotli-compressed (Skathi's scripts/css-engine.ts, its development profile; needs the wasm32
// target rust-toolchain.toml names). It is built per machine, so it is kept beside the tarballs
// (.skathi/sk-css.wasm.br) and copied into the installed CLI by the app's `postinstall`.
//
//   node src-skathi/scripts/fetch-skathi.mjs          # no-op when .skathi/REV already matches
//   node src-skathi/scripts/fetch-skathi.mjs --force
//   node src-skathi/scripts/fetch-skathi.mjs --force --update-lock   # after moving the pin
//
// Every run ends by comparing the tarballs' sha512 with app/bun.lock and stops on a difference.
// bun does not: with its cache warm, `bun install --frozen-lockfile` keeps installing the package
// the lock last named even when the tarball beside it no longer matches (KNOWN-ISSUES.md, "Reproducible Skathi package tarballs"), and an
// app built that way refuses to start against the new Skathi crate. bun also keeps an unpacked copy of
// a `file:` tarball under a key made from its path, not its content, and reinstalls that copy even
// with `--force`; so a new pack drops those copies, and so does a run that finds the installed
// package's version differing from the tarball's.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants, gunzipSync, gzipSync } from 'node:zlib';

/** 1985-10-26, the time npm itself gives packed files. */
const EPOCH = new Date('1985-10-26T08:15:00Z');

/** Every entry under `rel` (relative to `root`), directories first, in byte order. */
function walk(root, rel) {
  const out = [rel];
  for (const e of readdirSync(join(root, rel), { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const child = `${rel}/${e.name}`;
    if (e.isDirectory()) out.push(...walk(root, child));
    else out.push(child);
  }
  return out;
}

/**
 * A ustar archive of `entries` (paths relative to `root`, directories first), written here rather
 * than by the system's `tar`. Windows' bsdtar records 0666/0777 (NTFS has no mode bits) and a Mac's
 * can add pax headers for extended attributes, so the same files packed to two hashes and bun.lock
 * could hold only one of them (KNOWN-ISSUES.md, "Reproducible Skathi package tarballs"). Every field is fixed here: the modes git checks
 * out (0644, 0755 for a directory; Skathi's packages hold no executable), uid/gid 0 with no owner
 * names, one time, the entries in the order given.
 */
function ustar(root, entries) {
  const blocks = [];
  const put = (h, off, len, value) => h.write(value, off, len, 'latin1');
  const octal = (n, len) => n.toString(8).padStart(len - 1, '0') + '\0';
  const mtime = Math.floor(EPOCH.getTime() / 1000);
  for (const entry of entries) {
    const full = join(root, entry);
    const dir = statSync(full).isDirectory();
    const name = Buffer.from(dir ? `${entry}/` : entry, 'utf8');
    let prefix = Buffer.alloc(0);
    let base = name;
    if (name.length > 100) {
      // Split at a slash: the prefix takes up to 155 bytes, the rest up to 100.
      const cut = name.lastIndexOf(0x2f, 155);
      if (cut <= 0 || name.length - cut - 1 > 100) throw new Error(`${entry}: too long for a ustar name`);
      prefix = name.subarray(0, cut);
      base = name.subarray(cut + 1);
    }
    const body = dir ? Buffer.alloc(0) : readFileSync(full);
    const h = Buffer.alloc(512);
    base.copy(h, 0);
    put(h, 100, 8, octal(dir ? 0o755 : 0o644, 8));
    put(h, 108, 8, octal(0, 8));
    put(h, 116, 8, octal(0, 8));
    put(h, 124, 12, octal(body.length, 12));
    put(h, 136, 12, octal(mtime, 12));
    h.fill(0x20, 148, 156);
    put(h, 156, 1, dir ? '5' : '0');
    put(h, 257, 6, 'ustar\0');
    put(h, 263, 2, '00');
    put(h, 329, 8, octal(0, 8));
    put(h, 337, 8, octal(0, 8));
    prefix.copy(h, 345);
    let sum = 0;
    for (const b of h) sum += b;
    put(h, 148, 8, `${sum.toString(8).padStart(6, '0')}\0 `);
    blocks.push(h, body, Buffer.alloc((512 - (body.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return Buffer.concat(blocks);
}

const here = dirname(fileURLToPath(import.meta.url));
const skathiRoot = join(here, '..');
const out = join(skathiRoot, 'app', '.skathi');
const targetDir = process.env.CARGO_TARGET_DIR ?? join(skathiRoot, 'target');

const cargo = readFileSync(join(skathiRoot, 'Cargo.toml'), 'utf8');
const pin = /^skathi\s*=\s*\{[^}]*rev\s*=\s*"([0-9a-f]{40})"/m.exec(cargo)?.[1];
if (!pin) throw new Error('src-skathi/Cargo.toml: no `skathi = { git = …, rev = "<40 hex>" }` line');

/**
 * Holds app/bun.lock to the tarballs in `out`: with `--update-lock` writes their sha512 into it,
 * otherwise exits 1 when either differs.
 */
function checkLock() {
  const lockPath = join(skathiRoot, 'app', 'bun.lock');
  let lock = readFileSync(lockPath, 'utf8');
  let bad = 0;
  for (const name of ['cli', 'runtime']) {
    const have = `sha512-${createHash('sha512').update(readFileSync(join(out, `skathi-native-${name}.tgz`))).digest('base64')}`;
    const re = new RegExp(`("@skathi-native/${name}": \\["@skathi-native/${name}@[^\\n]*?)"(sha512-[A-Za-z0-9+/=]+)"`);
    const m = re.exec(lock);
    if (!m) throw new Error(`app/bun.lock: no integrity for @skathi-native/${name}`);
    if (m[2] === have) continue;
    if (process.argv.includes('--update-lock')) {
      lock = lock.replace(re, (_, head) => `${head}"${have}"`);
      console.log(`skathi packages: bun.lock now holds ${name} ${have.slice(0, 20)}…`);
    } else {
      console.error(`skathi packages: ${name} is ${have}\n  but app/bun.lock holds ${m[2]}`);
      bad++;
    }
  }
  if (process.argv.includes('--update-lock')) writeFileSync(lockPath, lock);
  if (bad) {
    console.error('skathi packages: the tarballs and bun.lock disagree. After moving the pin, run with --force --update-lock and commit bun.lock; otherwise the packer or the checkout differs from the machine that wrote the lock (KNOWN-ISSUES.md, "Reproducible Skathi package tarballs").');
    process.exit(1);
  }
}

/** `package/package.json` of a tarball this script wrote. */
function manifestOf(tgz) {
  const tar = gunzipSync(readFileSync(tgz));
  for (let off = 0; off + 512 <= tar.length; ) {
    const h = tar.subarray(off, off + 512);
    if (h[0] === 0) break;
    const cstr = (a, b) => h.subarray(a, b).toString('latin1').replace(/\0.*$/s, '');
    const name = [cstr(345, 500), cstr(0, 100)].filter(Boolean).join('/');
    const size = parseInt(cstr(124, 136), 8) || 0;
    if (name === 'package/package.json') return JSON.parse(tar.subarray(off + 512, off + 512 + size).toString('utf8'));
    off += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error(`${tgz}: no package/package.json`);
}

/** Removes bun's unpacked copies of the two packages and the app's installed ones. */
function dropBunCopies(why) {
  let cache;
  try {
    cache = execFileSync('bun', ['pm', 'cache'], { cwd: join(skathiRoot, 'app'), stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
  } catch {
    return; // no bun here: nothing of bun's to drop
  }
  if (!cache || !existsSync(cache)) return;
  let dropped = 0;
  for (const e of readdirSync(cache)) {
    const manifest = join(cache, e, 'package.json');
    if (!e.startsWith('@T@') || !existsSync(manifest)) continue;
    const { name } = JSON.parse(readFileSync(manifest, 'utf8'));
    if (name !== '@skathi-native/cli' && name !== '@skathi-native/runtime') continue;
    rmSync(join(cache, e), { recursive: true, force: true });
    dropped++;
  }
  rmSync(join(skathiRoot, 'app', 'node_modules', '@skathi-native'), { recursive: true, force: true });
  console.log(`skathi packages: ${why}; dropped ${dropped} cached cop${dropped === 1 ? 'y' : 'ies'} in bun's cache, run bun install again`);
}

/** The installed packages are the tarballs' (by version), or bun's copies go. */
function checkInstalled() {
  for (const name of ['cli', 'runtime']) {
    const installed = join(skathiRoot, 'app', 'node_modules', '@skathi-native', name, 'package.json');
    if (!existsSync(installed)) continue;
    const want = manifestOf(join(out, `skathi-native-${name}.tgz`)).version;
    const have = JSON.parse(readFileSync(installed, 'utf8')).version;
    if (have !== want) return dropBunCopies(`@skathi-native/${name} ${have} is installed, the tarball is ${want}`);
  }
}

const stamp = join(out, 'REV');
if (!process.argv.includes('--force') && existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === pin) {
  console.log(`skathi packages: up to date (${pin.slice(0, 8)})`);
  checkLock();
  checkInstalled();
  process.exit(0);
}

// `cargo metadata` fetches the git dependency (with its submodules) and says where it put it.
const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version', '1', '--manifest-path', join(skathiRoot, 'Cargo.toml')], {
    cwd: skathiRoot,
    maxBuffer: 256 << 20,
    stdio: ['ignore', 'pipe', 'inherit'],
  }).toString(),
);
const crate = metadata.packages.find((p) => p.name === 'skathi' && String(p.source).includes(pin));
if (!crate) throw new Error(`cargo metadata: no skathi package at ${pin}`);
let checkout = dirname(crate.manifest_path);
while (!existsSync(join(checkout, 'packages', 'cli', 'package.json'))) {
  const up = dirname(checkout);
  if (up === checkout) throw new Error(`${crate.manifest_path}: no packages/cli above it`);
  checkout = up;
}
console.log(`skathi packages: from cargo's checkout of ${pin.slice(0, 8)}`);

const work = mkdtempSync(join(tmpdir(), 'skathi-'));
try {
  // Only what git tracks: a file left in the checkout (a `node_modules` from an install there, an
  // editor's file) would otherwise go into one machine's tarball and not another's.
  for (const name of ['runtime', 'cli']) {
    const tracked = execFileSync('git', ['-C', checkout, 'ls-files', '-z', '--', `packages/${name}`], { maxBuffer: 64 << 20 })
      .toString()
      .split('\0')
      .filter(Boolean);
    if (!tracked.length) throw new Error(`${checkout}: git tracks nothing under packages/${name}`);
    for (const file of tracked) {
      const to = join(work, name, 'package', file.slice(`packages/${name}/`.length));
      mkdirSync(dirname(to), { recursive: true });
      cpSync(join(checkout, file), to);
    }
  }

  console.log('skathi packages: building the CSS checker (wasm32, Skathi\'s development profile)');
  execFileSync(
    'cargo',
    [
      'build', '--locked', '--profile', 'css-dev',
      ...['inherits="release"', 'lto=false', 'codegen-units=256', 'incremental=true'].flatMap((s) => ['--config', `profile.css-dev.${s}`]),
      '--target', 'wasm32-unknown-unknown', '-p', 'sk-css', '--example', 'css-check',
    ],
    { cwd: checkout, stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, CARGO_TARGET_DIR: targetDir } },
  );
  const wasm = readFileSync(join(targetDir, 'wasm32-unknown-unknown', 'css-dev', 'examples', 'css_check.wasm'));
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'sk-css.wasm.br'), brotliCompressSync(wasm, { params: { [constants.BROTLI_PARAM_QUALITY]: 4 } }));
  for (const name of ['runtime', 'cli']) {
    const dir = join(work, name, 'package');
    const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    // `workspace:*` means nothing outside Skathi's own workspace; the app installs the runtime itself.
    for (const field of ['dependencies', 'devDependencies', 'peerDependencies']) {
      for (const [dep, range] of Object.entries(manifest[field] ?? {})) {
        if (String(range).startsWith('workspace:')) delete manifest[field][dep];
      }
    }
    // devDependencies stay: an installer ignores them in a dependency, and the CLI reads its own
    // list (the versions it scaffolds with).
    delete manifest.scripts;
    writeFileSync(join(dir, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
    // A plain tarball in npm's layout (`package/…`): `files` is not applied, so nothing the build
    // reads by path is left behind. Reproducible on any machine: sorted entries, `ustar`, and a
    // gzip header without a time.
    const root = join(work, name);
    // The gzip header's OS byte is whatever system Node's zlib was built for (0x13 on macOS, 0x0a on
    // Windows): the only byte by which the two machines' tarballs differed. 0xff, "unknown", on all.
    const gz = gzipSync(ustar(root, walk(root, 'package')), { level: 9 });
    gz[9] = 0xff;
    writeFileSync(join(out, `skathi-native-${name}.tgz`), gz);
  }
  writeFileSync(stamp, pin + '\n');
  console.log(`skathi packages: ${pin.slice(0, 8)} packed into ${out}`);
  checkLock();
  dropBunCopies('new tarballs');
} finally {
  rmSync(work, { recursive: true, force: true });
}

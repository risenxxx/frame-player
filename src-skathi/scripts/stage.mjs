// Copies the files v2 takes from v1 as they are, so that nothing is kept twice in git:
//   static/                                  → src-skathi/app/public/   (the payload's public dir: fonts, icons)
//   src-tauri/{lua,shaders}, LICENSE         → src-skathi/resources/    (what Tauri calls the resource dir)
// and version 2's own notices, src-skathi/THIRD-PARTY-NOTICES.md (scripts/gen-notices.mjs), beside them:
// version 1's file describes version 1's libraries, not what version 2 ships.
// Both targets are ignored by git and rebuilt whole on every run. Skathi wants the public dir inside
// the app root, and the host reads its resources from one folder, as an installed app would.
import { cpSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const skathi = join(dirname(fileURLToPath(import.meta.url)), '..');
const repo = join(skathi, '..');

const copy = (from, to) => cpSync(join(repo, from), to, { recursive: true });

const pub = join(skathi, 'app', 'public');
rmSync(pub, { recursive: true, force: true });
copy('static', pub);

const res = join(skathi, 'resources');
rmSync(res, { recursive: true, force: true });
mkdirSync(res, { recursive: true });
copy('src-tauri/lua', join(res, 'lua'));
copy('src-tauri/shaders', join(res, 'shaders'));
copy('LICENSE', join(res, 'LICENSE'));
copy('src-skathi/THIRD-PARTY-NOTICES.md', join(res, 'THIRD-PARTY-NOTICES.md'));
console.log('staged: app/public, resources');

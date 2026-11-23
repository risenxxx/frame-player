# Shipping the version 2 beta beside version 1

A proposal: how version 2 (the interface on Skathi, [ROADMAP-v2.md](ROADMAP-v2.md))
reaches viewers as a beta while version 1 keeps shipping, how the beta updates
itself, how a viewer's state crosses over, and how version 1 tells its viewers
that the beta exists. Nothing here is built yet. [ROADMAP-v2.md](ROADMAP-v2.md)
V2-2 already decides Velopack and the shape of the final move; this file is the
step before it — **two players installed at once** — and says where that step
changes V2-2.

Every claim carries its source:

- **[src: file:line]** — read in the source (version 1 at `main`, release
  1.49.0; the Skathi checkout under the prototype; the prototype itself);
- **[doc: URL]** — read in a project's documentation;
- **[measured]** — looked at on a real installation (file names and sizes
  only, never contents);
- **[inferred]** — follows from the above, not checked;
- **[not checked]** — said so that nobody takes it as known.

Placeholders: `<v2-bucket>` is the new storage bucket, `<v2-updates-host>` its
public address, `<v1-updates-host>` today's update host, `<site>` the landing
page. No real name of a bucket, account or machine belongs in this file.

## Decided

The owner's answers of 2026-11-23 to the five questions the first draft left
open. Everything below follows from them.

1. **The beta identity is temporary.** After graduation, changes go straight
   to stable, as they do for version 1 today. A permanent preview edition
   would fall behind stable and cost CI time to keep in step. At graduation the
   beta hands its viewers to stable version 2 and retires; any later
   pre-release is the `beta` channel of the stable package.
2. **Version 2 reads version 1's data itself, during onboarding.** There is no
   export from version 1. The Windows LevelDB reader stays the main risk.
3. **Torrents**: the choice was between a shared download folder and moving the
   selected torrents during onboarding. This file evaluates both and
   recommends the move (see "Torrents").
4. **No Windows code-signing certificate for now.** The beta ships unsigned on
   Windows; macOS stays signed and notarised.
5. **Version 2 lives in this repository on a separate branch** until it
   becomes the stable version, with its own release workflow.

Answers of 2026-11-23 to the first revision's open questions:

6. **If the Windows LevelDB reader fails, the beta still ships on Windows**,
   importing files and torrents only. History and settings follow in a later
   beta update, through the repeat import.
7. **At graduation, stable keeps the `Frame Player 2` subfolder** inside a
   chosen root as one of its roots. Torrents are not moved back.
8. **Opting into later pre-releases is a settings switch only.** A separate
   download link exists only for the beta, before stable version 2 ships.
9. **The release workflow copies version 2's release notes to `main`**, so
   the site never reads the `v2` branch.
10. **The two optional version 1 torrent fixes go to `docs/v1-backports.md`.**
    They are items 6 and 7 of [v1-backports.md](v1-backports.md).

## The recommended shape, in one paragraph

During the beta, version 2 is installed as **"Frame Player Beta"**, a
**temporary** second application: bundle ID `app.frameplayer.beta`, Velopack
package ID `FramePlayerBeta`, data folder `app.frameplayer.beta`. Velopack
installs and updates it from **its own bucket**. It never becomes the default
for anything and never claims `frameplayer://`, so version 1 keeps every
association it has. **Its onboarding screen finds version 1** (installed or
portable), shows what it found and imports it. Importing only copies; version 1's
data is never written, apart from the torrent folders the viewer chooses to
**move**. Version 1 later gets a **"Version 2 beta"** button, driven by a manifest
that can switch it off remotely. At graduation **stable version 2 takes version
1's identity** (`app.frameplayer`, package `FramePlayer`), so v1 → v2 is an
in-place upgrade. The beta receives no new package: it learns from a signed
status file that stable is out, hands its viewer over, and stable imports the
beta's data with the same importer it uses for version 1. Later pre-releases
go to the `<os>-<arch>-beta` channel of `FramePlayer`. Every release in the
whole lifecycle builds **one package per platform**.

## What version 1 is today

### Hosting and the update manifest

- The manifest is one `latest.json` at `<v1-updates-host>/latest.json`, named
  in the bundle configuration [src: src-tauri/tauri.conf.json:26] and signed
  with minisign [src: src-tauri/tauri.conf.json:27]. The private key is the
  repository secret `TAURI_SIGNING_PRIVATE_KEY`, passwordless
  [src: .github/workflows/release.yml:256]; it exists nowhere else (project
  memory, and [src: docs/rules/build-and-release.md], the "Releases" line).
- The bucket is Cloudflare R2, written with the AWS CLI against the R2 S3
  endpoint; the bucket name and public URL are repository *variables*
  (`R2_BUCKET_UPDATES`, `R2_PUBLIC_URL_UPDATES`), the credentials secrets
  [src: .github/workflows/release.yml:629-657]. Binaries go up first and
  `latest.json` last, with `no-cache` [src: .github/workflows/release.yml:647-657],
  then the bucket is pruned to five versions
  [src: .github/workflows/release.yml:689-702]. Release assets are kept on the
  GitHub Release forever [src: .github/workflows/release.yml:706-707].
- Manifest keys: `darwin-aarch64` (macOS is Apple Silicon only
  [src: .github/workflows/release.yml:83-96]), `windows-x86_64`,
  `windows-aarch64` (NSIS installers) and `windows-x86_64-zip`,
  `windows-aarch64-zip` (the in-place payload)
  [src: .claude/rules/build-and-release.md, the Windows architecture line].
- The release is gated on the version in `tauri.conf.json` changing
  [src: .github/workflows/release.yml:38-42]; notes come from
  `changelog/<version>.md`, written by `set-version` from `.changeset/`
  [src: docs/rules/build-and-release.md, the release-notes line].
- The site's download buttons are resolved from `latest.json` at build time
  and it shows only releases the manifest says are out
  [src: site/src/releases.ts:13-21; .github/workflows/release.yml:781-782].

### Signing

- **macOS**: Developer ID Application certificate, hardened runtime,
  notarised and stapled; the native libraries are signed *before* the build,
  the updater archive repacked after stapling, the disk image built with
  `dmgbuild` and notarised separately [src: docs/distribution.md:13-32;
  .github/workflows/release.yml:261-385]. Six optional secrets; without them
  the build is ad-hoc [src: docs/rules/build-and-release.md, "Releases" line].
- **Windows**: **no Authenticode signature** — `release.yml` signs nothing but
  the update payload with minisign [src: .github/workflows/release.yml:425;
  grep for `signtool` finds nothing]. SmartScreen reputation is discussed and
  not bought [src: docs/distribution.md:151-170].

### Identity

| | Version 1 | Source |
|---|---|---|
| Bundle identifier | `app.frameplayer` | [src: src-tauri/tauri.conf.json:5], mirrored as `APP_IDENTIFIER` [src: src-tauri/src/lib.rs:237] |
| Product name / macOS bundle | `Frame Player`, `Frame Player.app` | [src: src-tauri/tauri.conf.json:3] |
| Windows install folder | `%LOCALAPPDATA%\Frame Player` (NSIS, per user, folder choosable) | [src: .claude/rules/build-and-release.md, the `-CompareWith` command] |
| URL scheme | `frameplayer` (deep-link plugin) | [src: src-tauri/tauri.conf.json:18-23] |
| `magnet:` | a *candidate*, never the scheme list: `Info.plist` on macOS, `RegisteredApplications` → `FramePlayer.Magnet` on Windows | [src: .claude/rules/sources-and-privacy.md:45] |
| Windows file types | one ProgId per extension, `FramePlayer.<ext>`, capabilities at `Software\FramePlayer\Capabilities`, registered as "Frame Player"; markers at `Software\FramePlayer\Associations` | [src: src-tauri/src/shell_handler.rs:153-186] |
| Single instance | `tauri-plugin-single-instance`, keyed on the identifier; `FP_NO_SINGLE_INSTANCE` skips it | [src: src-tauri/src/lib.rs:1310-1325] |
| Keychain / Credential Manager | service `app.frameplayer.opensubtitles` | [src: src-tauri/src/opensubtitles.rs:536-555] |
| Autostart | none | [src: grep for `autostart` in `src-tauri/` finds nothing] |

### Where version 1 keeps its state

Every base directory goes through `portable::{config,data,cache}_dir`
[src: src-tauri/src/portable.rs:128-145], which for an ordinary installation
is Tauri's directory for the identifier and for a portable copy
`.data/{config,data,cache}` beside the executable
[src: src-tauri/src/portable.rs:20-31]. The web view's store is the platform
default for an installation — Tauri forces `LocalData/<identifier>` when no
data directory is passed [src: src-tauri/src/lib.rs:966-971] — and
`.data/webview` for a portable copy [src: src-tauri/src/lib.rs:990-992].

| Store | Windows (installed) | macOS | Format |
|---|---|---|---|
| Web view store (all `frameplayer.*` keys) | `%LOCALAPPDATA%\app.frameplayer\EBWebView\Default\Local Storage\leveldb\` | `~/Library/WebKit/app.frameplayer/WebsiteData/Default/<salt>/<salt>/LocalStorage/localstorage.sqlite3` | Windows: Chromium LevelDB [inferred: the standard WebView2 layout; the `EBWebView` folder is [src: docs/distribution.md:315]]; macOS: SQLite, one table `ItemTable(key TEXT UNIQUE, value BLOB)`, values UTF-16LE [measured: schema and value lengths] |
| `mpv.conf` | `%APPDATA%\app.frameplayer\mpv.conf` | `~/Library/Application Support/app.frameplayer/mpv.conf` | text, `key=value` [src: src-tauri/src/lib.rs:609-615; docs/distribution.md:312] |
| `install-count` | config dir | same | one word [src: src-tauri/src/install_count.rs:23-40] |
| Subtitle fonts | `%LOCALAPPDATA%\app.frameplayer\subtitle-fonts\` | `~/Library/Application Support/app.frameplayer/subtitle-fonts/` | font files [src: src-tauri/src/subtitle_fonts.rs:30-33] [measured: macOS folder present] |
| Own OpenSubtitles API key (optional) | data dir `opensubtitles.key` | same | text [src: src-tauri/src/opensubtitles.rs:60-66] |
| OpenSubtitles login | Credential Manager | login keychain | service `app.frameplayer.opensubtitles` [src: src-tauri/src/opensubtitles.rs:536-567] |
| yt-dlp | data dir `bin/` | same | binary [src: src-tauri/src/lib.rs:414] |
| Torrent session | cache dir `torrents/` (librqbit JSON persistence with fastresume, `.roots.json` naming the chosen root and every root seen) | `~/Library/Caches/app.frameplayer/torrents/` | JSON + content [src: src-tauri/src/torrent/session.rs:186-211; src-tauri/src/torrent/dirs.rs:48,75-95] |
| Torrent content | the chosen root, default the cache dir above | same | files; 128 GB on one Windows machine [src: docs/distribution.md:313] |
| Caches | cache dir `thumbs/`, `posters/`, `cast/`, `shaders/`, `subtitles/`, `logs/` | `~/Library/Caches/app.frameplayer/…` | regenerable [src: thumb_service.rs:686,1615; cast/mod.rs:466; lib.rs:577; opensubtitles.rs:1061; logs.rs:647] |

The **localStorage keys** in use at 1.49.0 [src: grep of `src/` for
`'frameplayer.*'`; [measured]: 24 of them present on one macOS installation]:

| Group | Keys | Defined at |
|---|---|---|
| History & privacy | `history` (enabled + excluded folders), `positions`, `resume`, `titles`, `links`, `torrents`, `subs` | history.svelte.ts:39-42,175,239,282-285 |
| Track memory | `tracks`, `tracks.folder`, `picture`, `audioOffsets` | history.svelte.ts:42,351,512; report.svelte.ts:236 |
| Hotkeys | `keys` (overrides only, on top of `ACTIONS`) | keys.svelte.ts:466 |
| Playback prefs | `volume`, `loop`, `amplify`, `normalise`, `sdrcolor`, `frameStep`, `seek`, `playlist`, `thumbs`, `subLift`, `sub2Place` | player/commands.ts:25,78,116; player/filters.ts:20,58; frame-step.svelte.ts:56; seek-steps.svelte.ts:27; playlist.svelte.ts:61; thumbs.svelte.ts:46; sub-shift.svelte.ts:68; tracks.svelte.ts:365 |
| Interface | `locale`, `window`, `timeRemaining`, `seekbarThickness` | i18n.svelte.ts:34; window-prefs.svelte.ts:52; seekbar.svelte.ts:84,123 |
| Torrents | `torrent` (network prefs) | torrent/prefs.svelte.ts:13 |
| Catalog | `catalog`, `indexer`, `indexerApiKey`, `tmdb`, `tmdbCdn`, `releaseSort`, `releaseHideDead`, `releaseFloor` | catalog.svelte.ts:87-99 |
| Casting | `castMode`, `castCacheCap`, `tv-hosts`, `tv-devices`, `airplay-mode`, `airplay-refusals` | cast/prefs.ts:5-6; cast/discovery.ts:101; report.svelte.ts:223; airplay-choice.ts:19-20 |
| Watch together | `relay`, `syncName` | sync/wire.svelte.ts:65-66 |
| Shell | `handler` (portable copy's registration switch) | portable.svelte.ts:22 |
| Other | `app` | redact.ts:49 |

(All paths under `src/lib/`. The names are in the code without the
`frameplayer.` prefix here only for width.)

### The pitfall issue #81 shows

A portable copy's main window is given `.data/webview`, but the hidden `veil`
window is not, so every portable launch writes into
`%LOCALAPPDATA%\app.frameplayer\EBWebView` — the installed player's profile
[src: issue #81; src-tauri/src/lib.rs:1008]. The lesson for this proposal:
**every path the beta touches must come from one function**, and a test must
hash version 1's folders before and after a beta run (see "Verification").

## What version 2 does with data today

- Skathi keeps `localStorage` as **one JSON file per area**,
  `localStorage.json` — `{"format": "vue-native/storage", "version": 1,
  "entries": [[key, value], …]}` — rewritten whole by rename
  [src: sk-master/crates/sk-storage/src/file.rs:1-45]. Optionally sealed
  (`localStorage.sealed`): DPAPI on Windows, a per-app key in the login
  keychain on macOS; encryption is sticky once on
  [src: sk-master/crates/sk-storage/src/crypt.rs:1-14; src/lib.rs:5-13].
- Default folder: `%LOCALAPPDATA%\<app name>\storage` on Windows, the app name
  being `SK_APP_NAME`, the name given at install, or the exe's stem
  [src: sk-master/crates/sk-storage/src/lib.rs:5-7]; `~/Library/Application
  Support/<name>/storage` on macOS [src: ui-port/REPORT.md:131]. The embedder
  can set the whole profile root with `Builder::data_dir` (absolute; `SK_DATA_DIR`
  overrides) [src: sk-master/crates/sk-app/src/embed.rs:366-371]; the prototype
  sets it to `./data` [src: childlayer/mac-ui14/src/main.rs:373].
- `Builder::prepare_data_dir` runs **after single-instance handling, before
  any storage or VM initialisation**, and an error or panic there **stops
  startup** [src: sk-master/crates/sk-app/src/embed.rs:378-386]. That is the
  migration's natural home — and why the migration must swallow its own errors.
- Two writers on one folder are last-writer-wins
  [src: sk-master/crates/sk-storage/src/lib.rs:11-12].
- The prototype's bundle ID is a placeholder, `com.example.fpui`
  [src: childlayer/mac-ui14/app/sk.config.json].
- **The interface is the same Svelte code**, so version 2 reads and writes the
  same `frameplayer.*` keys [src: ui-port/REPORT.md:131, "works; existing users'
  WebKit storage would need a migration"]. That is what makes the migration
  mostly a *copy* rather than a translation.
- V2-3 moves settings off `localStorage` into an application-owned, versioned
  file [src: docs/ROADMAP-v2.md:167-171]. The migration therefore writes two
  targets: settings into V2-3's file (only values that differ from the default,
  since V2-3 keeps "only what the viewer actually changed"), everything else
  into Skathi's `localStorage.json` under the same key.

## Identity: two players on one machine, for a while

### The recommendation

| | v1 (unchanged) | v2 beta (temporary) | v2 stable (from graduation) |
|---|---|---|---|
| Display name | Frame Player | **Frame Player Beta** | Frame Player |
| macOS bundle ID | `app.frameplayer` | `app.frameplayer.beta` | `app.frameplayer` |
| macOS bundle | `Frame Player.app` | `Frame Player Beta.app` | `Frame Player.app` |
| Velopack `packId` | — | `FramePlayerBeta` | `FramePlayer` |
| Velopack channels | — | `win-x64-beta`, `osx-arm64-beta` | `<os>-<arch>-stable`, and `<os>-<arch>-beta` for later pre-releases |
| Windows install folder | `%LOCALAPPDATA%\Frame Player` | `%LOCALAPPDATA%\FramePlayerBeta` [doc: https://docs.velopack.io/packaging/installer] | `%LOCALAPPDATA%\FramePlayer` |
| Data folder | `…\app.frameplayer` | `…\app.frameplayer.beta` | `…\app.frameplayer` (v1's own) |
| Single-instance key | `app.frameplayer` | `app.frameplayer.beta` | `app.frameplayer` |
| Windows ProgIds | `FramePlayer.<ext>`, `FramePlayer.Magnet` | `FramePlayerBeta.<ext>`, `FramePlayerBeta.Magnet` | `FramePlayer.<ext>` (taken over) |
| Registered application | "Frame Player" | "Frame Player Beta" | "Frame Player" |
| `frameplayer://` | yes | **no** | yes |
| Keychain service | `app.frameplayer.opensubtitles` | `app.frameplayer.beta.opensubtitles` | `app.frameplayer.opensubtitles` |
| Icon | today's | today's with a "β" mark | today's |

**Why the beta cannot share version 1's identity.** Everything keyed on
`app.frameplayer` would then belong to two programs that write it
differently: the data folders, the single-instance guard, the keychain entry,
and Launch Services' idea of which app is which [inferred from the identity
table under "What version 1 is today"]. Skathi's storage is last-writer-wins
on a shared folder [src: sk-storage/src/lib.rs:11-12], and two librqbit
sessions over one store would each hold the files
[src: src-tauri/src/torrent_storage.rs:1-20].

**Why stable takes version 1's identity.** The v1 → v2 move is then an
upgrade, not a second application. On macOS the same bundle ID keeps the Dock
item, the "Open with" default, the Local Network permission casting needs, and
the login keychain entry [inferred: all four are keyed on the bundle ID]. The
data folder is v1's own, so `mpv.conf`, `subtitle-fonts/` and the torrent store
are already where v2 looks; only the web view's store is converted. That is
V2-2's bridge, unchanged [src: docs/ROADMAP-v2.md:122-133].

### One identity from day one, compared

The alternative is `FramePlayer` / `app.frameplayer` from the first beta, on the
`beta` channel, with no hand-off at all.

| | Temporary beta identity (recommended) | One identity from day one |
|---|---|---|
| Windows during the beta | Two separate products, no overlap | Works: Velopack's `%LOCALAPPDATA%\FramePlayer` and NSIS's `%LOCALAPPDATA%\Frame Player` are different folders [inferred]. But the data folder (`app.frameplayer`) **is** v1's, so the beta would write v1's `mpv.conf` and torrent store while v1 runs. Avoiding that means giving the beta another data folder, which is the hand-off problem again |
| macOS during the beta | Two bundle IDs | Two installed bundles with **one** bundle ID: Launch Services picks one for "Open with" and `open -b`; both write the `app.frameplayer` preferences domain and the saved window state; the keychain entry is shared [inferred] |
| Graduation | One hand-off for beta viewers (below), with no extra package | None for beta viewers; v1 viewers go through the bridge either way |
| Packages per release | One | One |

One identity is cleaner **only on Windows**. On macOS it puts two
applications under one bundle ID for the whole beta, which is the collision this
file exists to avoid. **Recommendation: the temporary beta identity.** The
hand-off costs code once, not a package per release.

A middle way was considered and rejected: package `FramePlayer` from day one,
with the beta's bundle ID and data folder switched to stable's by an
ordinary update at graduation. On macOS Velopack updates a bundle **in place**
[doc: https://docs.velopack.io/packaging/operating-systems/macos], so the
graduated beta would still be `Frame Player Beta.app` on disk. The bridge
then installs a second `Frame Player.app` with the same bundle ID and
package ID: two bundles, one identity, the collision above.

### What the beta must not do

- **Never become a default by itself.** On Windows it registers
  `FramePlayerBeta.*` ProgIds and a `RegisteredApplications` entry, so it
  appears in *Open with* and Default apps. It never writes
  `HKCU\Software\Classes\.<ext>` defaults [inferred: version 1's own candidacy
  scheme, src: src-tauri/src/shell_handler.rs:1-60]. These keys are written in
  Velopack's after-install hook and removed in its before-uninstall hook,
  because Velopack does not register associations itself [doc:
  https://github.com/velopack/velopack/issues/29]. On macOS the beta declares
  document types with `LSHandlerRank` `Alternate` [inferred; the rank v1 uses
  for audio, src: src-tauri/tauri.conf.json:78].
- **Never claim `frameplayer://`.** When two bundles declare one scheme, Launch
  Services chooses between them, and we cannot predict which [inferred]. Invite
  links keep opening version 1; the beta joins a room by pasted code.
- **`magnet:`** stays a candidate on Windows, as in version 1
  [src: .claude/rules/sources-and-privacy.md:45]. On macOS the beta does not
  declare it.
- **It touches version 1's folders only through onboarding.** That means reading,
  plus renaming the torrent folders the viewer selected (see "Torrents"). No
  shared cache, no shared torrent root.
- **The install count** reports a beta installation as its own event, with
  the platform and a `beta` flag only [inferred from
  src: src-tauri/src/install_count.rs:23-40].

### The Windows portable copy

`vpk pack` also produces a portable zip that updates itself
[src: docs/ROADMAP-v2.md:60-63], offered as a second download as version 1
does. It keeps `.portable` and `.data/`. Issue #81's lesson applies from the
first commit: one function resolves every path, and verification hashes
`%LOCALAPPDATA%` before and after a run.

## Hosting

### Buckets and layout

A new R2 bucket, `<v2-bucket>`, served at `<v2-updates-host>`, with its own
credentials. Version 1's bucket, manifest, key and pruning are not touched.

```
<v2-bucket>/
├── beta/                         # one prefix per package ID
│   ├── win-x64/                  #   …and per platform; the channel name is
│   │   ├── releases.win-x64-beta.json    #   per OS/arch too, as Velopack requires
│   │   ├── releases.win-x64-beta.json.sig  # our ed25519 signature (V2-2)
│   │   ├── FramePlayerBeta-2.0.0-beta.4-full.nupkg
│   │   ├── FramePlayerBeta-2.0.0-beta.4-delta.nupkg
│   │   ├── FramePlayerBeta-win-Setup.exe
│   │   └── FramePlayerBeta-win-Portable.zip
│   ├── win-arm64/ …                      # only if v2 ships ARM64
│   └── osx-arm64/
│       ├── releases.osx-arm64-beta.json(.sig)
│       ├── FramePlayerBeta-2.0.0-beta.4-full.nupkg / -delta.nupkg
│       └── Frame Player Beta.dmg         # dmgbuild, as today; no .pkg (--noInst)
├── stable/                       # created at graduation, same shape
└── announce/
    ├── v1-beta-offer.json        # what version 1's button reads (below)
    └── beta-status.json(.sig)    # active | ended: the beta's hand-off (Graduation)
```

The exact file names are Velopack's [not checked here; see "Velopack"]. One
folder per OS/arch is a choice, not a requirement: it keeps the per-OS feeds
from colliding when the channel is the same word on both platforms, and keeps
pruning simple.

**Retention**: keep the last *N* full packages per feed plus their deltas;
Velopack needs the previous release present to build a delta (V2-2) — CI
fetches it back from the bucket before `vpk pack` [inferred; `vpk download`,
see "Velopack"]. Every release also goes to a GitHub Release marked
*pre-release*, which keeps the files forever, as version 1 does.

### Channels

- **During the beta**: `win-x64-beta` and `osx-arm64-beta` under
  `FramePlayerBeta`. Velopack wants one channel name per OS and architecture
  [doc: https://docs.velopack.io/packaging/channels].
- **From graduation**: `<os>-<arch>-stable` under `FramePlayer`, under the
  `stable/` prefix of the same bucket.
- **Later pre-releases**: `<os>-<arch>-beta` under `FramePlayer`. Velopack
  supports several channels per package: each channel has its own
  `releases.<channel>.json` [doc: https://docs.velopack.io/packaging/channels].
  An installed copy changes channel with `UpdateOptions { ExplicitChannel,
  AllowVersionDowngrade }`, and the app must then apply the update itself,
  because auto-apply on startup does not install an equal or lower version
  [doc: https://docs.velopack.io/integrating/switching-channels;
  https://docs.rs/velopack/latest/velopack/struct.UpdateOptions.html].
  Opting in is a **settings switch only**; there is no separate download
  link for pre-releases (decision 8). The beta's own download link exists only
  until stable version 2 ships.
  A copy that opted into pre-releases checks **both** feeds
  and takes the higher version, so a stable release reaches it without being
  published twice [inferred: two `UpdateManager`s, one per channel; SemVer
  orders `2.1.0-beta.3` below `2.1.0`]. Deltas are built within one channel,
  so moving between channels downloads a full package once [inferred from
  https://docs.velopack.io/packaging/deltas].

### The manifest and its signature

Velopack's own feed (`releases.<channel>.json`) is the manifest; we add no
second one for the updater. V2-2 already decides to sign that feed with
**a new ed25519 key** and verify it in our code before Velopack reads it
[src: docs/ROADMAP-v2.md:109-117]. This proposal agrees and adds two things:

- **A new key, not version 1's minisign key.** Different format, different
  blast radius; a leak of one does not reach the other.
- **The key gets an offline backup** (an encrypted copy outside GitHub; decided,
  and ROADMAP-v2.md V2-2 now says the same). Version
  1's key exists only as a GitHub secret (project memory), and losing it would
  strand every installed player; the beta is the moment to not repeat that.

### CI: a workflow on the v2 branch

Version 2 lives in this repository on its own branch (called `v2` here) until
it graduates (decision 5).

- **`release-v2.yml` exists only on the `v2` branch** and triggers on
  `push: branches: [v2]`. A push event runs the workflow file of the pushed
  commit [doc: https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#push,
  not re-read this session], so `main` carries nothing of it. Version 1's
  `release.yml`, `checks.yml` and `site.yml` trigger on `branches: [main]`
  [src: .github/workflows/release.yml:3-6; checks.yml:21-23; site.yml:6-8], so
  a push to `v2` never starts a version 1 release. The `v2` branch replaces
  `checks.yml` and `rust.yml` with its own gates.
- **The gate is "the tag for this version does not exist yet"**, version 1's
  `check-version` rule [src: .github/workflows/release.yml:12-13,44]. It is
  checked on every push to `v2`, which costs one Ubuntu job of seconds. A
  release that failed on a compile error is retried by the commit that fixes
  it, with no version burned. A dead runner is retried with "Re-run failed
  jobs". `workflow_dispatch` is not relied on: GitHub lists it only for
  workflows on the default branch [doc: same page, `workflow_dispatch`, not
  re-read].
- **Tags**: version 1 tags `v1.x.y` [src: .github/workflows/release.yml:669;
  `git tag`]. Version 2 tags `v2.0.0-beta.N`, then `v2.0.0`. The major number
  keeps the two apart without a prefix. **Every beta GitHub Release is marked
  pre-release**, because the site's "all releases" link is `/releases/latest`
  [src: site/src/release.ts:35], and GitHub leaves pre-releases out of "latest"
  [inferred]. The first v2 release that is not a pre-release is stable 2.0.0,
  and it is meant to become "latest".
- **Concurrency group** `release-v2`, so a v2 and a v1 release can run at once
  but not two of either. The two publish to different buckets.
- **Matrix**: `osx-arm64` on `macos-26`, `win-x64` on `windows-latest`, and
  `win-arm64` only if version 2 targets it. Each job runs: build → sign the
  libraries (macOS, today's script) → `vpk download` the previous release →
  `vpk pack` (macOS signing and notarisation through `vpk`; Windows unsigned) →
  sign the feed with the ed25519 key → upload packages, then the feed last
  with `no-cache` → prune. `vpk` needs the .NET 8 SDK on every runner [doc:
  https://docs.velopack.io/getting-started/rust].
- **Secrets**: the Apple ones are reused (same team, same Developer ID). New:
  `V2_FEED_SIGNING_KEY`, and R2 credentials scoped to `<v2-bucket>`. Variables:
  `R2_BUCKET_V2`, `R2_PUBLIC_URL_V2`.
- **The site lives on `main`** and builds from `main`
  [src: .github/workflows/site.yml:6-11]. The `/beta` page is a change to `main`.
  `release-v2.yml` first copies the release notes to `main` (below), then asks
  for a rebuild with `gh workflow run site.yml --ref main`.
  It must name `main` explicitly: version 1's step passes `GITHUB_REF_NAME`
  [src: .github/workflows/release.yml:786-791], which on `v2` would build the
  site from the wrong tree.
- **No Homebrew cask for the beta.** Version 1's release bumps one
  [src: .github/workflows/release.yml:776-779]. A beta cask can come later, if
  asked for.

**Changesets and changelog.** On `v2`: `.changeset-v2/` and `changelog-v2/`.
`set-version` and `changelog.mjs` take the directory as a parameter on that
branch. The format is `.changeset/README.md`'s, unchanged. Version 1's
`.changeset/` files that arrive through merges are ignored by v2's tooling. A
version 1 fix that v2 viewers will notice gets its own v2 changeset. After a successful
v2 release, `release-v2.yml` **copies** `changelog-v2/<version>.md` to `main`
(a commit touching only `changelog-v2/`, made with the workflow's token) and
then asks for the site rebuild, so the site builds from `main` alone and
never reads the `v2` branch (decision 9). Merges of `main` into `v2` then
carry the same file back unchanged [inferred]. `changelog:check` on `main`
must accept the folder without a matching version 1 version [inferred].
At graduation, `changelog-v2/` merges into `changelog/` and `.changeset/`
becomes v2's again.

**How `main`'s fixes reach `v2`.** Merge `main` into `v2` after every version 1
release. Merge, never rebase: the project does not rewrite pushed history
(project rule). Files `v2` deleted (`update.rs`, `installer-hooks.nsh`,
`webview2.rs`, the Tauri configuration) produce modify/delete conflicts, and
those are resolved by keeping the deletion. A fix in code both versions share
(the Svelte interface, the torrent and casting crates) therefore arrives without
being written twice. Nothing goes from `v2` back to `main` before graduation,
with **one explicit exception**: the release workflow's copy of
`changelog-v2/<version>.md` to `main` (decision 9), which touches nothing but
that folder;
improvements that version 1 could also use are written down in
[v1-backports.md](v1-backports.md) (project decision of 2026-11-23). At graduation `v2` is
merged into `main`, and `main`'s workflows become v2's.

### CI cost of the whole lifecycle

Measured on version 1's last successful release: about **12 minutes** of wall
clock, with three build jobs of about 10 minutes each (macOS arm64, Windows x64,
Windows arm64) plus a 1-minute publish [measured: `gh run view` of the run of
2026-11-20]. Version 2 adds the .NET SDK and `vpk pack` to each job, a few
minutes [inferred]. Standard GitHub-hosted runners are free for a public
repository [doc: https://docs.github.com/en/billing/managing-billing-for-your-products/managing-billing-for-github-actions/about-billing-for-github-actions,
not re-read], so the cost is queue time and wall clock, not money.

| Phase | Per release | Packages built |
|---|---|---|
| Beta | one v2 release (`FramePlayerBeta`, beta channels); version 1's own releases as today | **1 per platform** |
| Graduation | one v2 release (`FramePlayer`, stable channels). The beta gets **no** package: `announce/beta-status.json` flips to `ended` (below). Version 1 gets its bridge as an ordinary v1 release | **1 per platform** for v2, plus v1's normal build |
| After | stable releases on `stable`, pre-releases on `beta` of the same package; a pre-release opt-in reads both feeds | **1 per platform** |

No step builds two v2 packages for one release. The hand-off code ships
inside ordinary beta releases, dormant until the status file says otherwise.

### Download page and URLs

- `<site>/beta` (and `/ru/beta`, `/zh/beta`) on `main`. Its buttons are resolved
  at build time from `<v2-updates-host>/beta/<platform>/releases.<platform>-beta.json`,
  the way the home page reads `latest.json` [src: site/src/releases.ts:13-21].
  It also carries the unsigned-Windows text below.
- Version 1's download URLs stay exactly as they are.
- Direct links, stable across releases:
  `<v2-updates-host>/beta/win-x64/FramePlayerBeta-win-Setup.exe`,
  `…/win-x64/FramePlayerBeta-win-Portable.zip`,
  `…/osx-arm64/Frame Player Beta.dmg` [inferred: Velopack names Setup without a
  version; to be confirmed in step 0].

## Velopack

What it does for this plan, read in its documentation and crate source in
November 2026. V2-2 read its source earlier; where the two agree this does not
repeat V2-2.

**Maturity and license.** MIT, a Rust rewrite of Clowd.Squirrel, ~2.5k stars,
one principal maintainer [doc: https://github.com/velopack/velopack;
https://github.com/clowd/Clowd.Squirrel]. The crate's latest stable is
**1.2.161** (2026-09-29), first published 2024-02-04
[doc: https://crates.io/api/v1/crates/velopack].

**Packaging a Rust app.** `vpk pack -u <packId> -v <version> -p <dir> -e <exe>`,
plus `--packTitle`, `--icon`, `--bundleId`/`--plist` on macOS
[doc: https://docs.velopack.io/getting-started/rust;
https://docs.velopack.io/reference/cli/content/vpk-osx]. `vpk` is a .NET tool:
**CI needs the .NET 8 SDK** although the app is Rust (same page). **macOS
packages can only be built on a Mac** [doc:
https://docs.velopack.io/packaging/cross-compiling]. `--packDir` accepts a
finished `.app` [doc: macOS page above], so the libmpv dylibs travel inside it
as today; nothing in the docs covers bundled dylibs beyond deep signing
[not checked]. Universal binaries are not documented [not checked]; moot while
macOS ships arm64 only [src: .github/workflows/release.yml:83-96].

**In the app.** `VelopackApp::build().run()` first thing in `main` (it may exit
or restart the process); `on_first_run` and `on_restarted` work on every OS;
`UpdateManager::new(source, options, locator)`, `check_for_updates`,
`download_updates(&info, progress)`, `wait_exit_then_apply_updates` (the
recommended quiet path) or `apply_updates_and_restart`; auto-apply on startup
is on by default; no async API [doc:
https://docs.rs/velopack/latest/velopack/struct.VelopackApp.html;
https://docs.rs/velopack/latest/velopack/struct.UpdateManager.html]. Skathi's
host is our own `main` [src: childlayer/mac-ui14/src/main.rs:368-373], so this
fits before `skathi::Builder`.

**Two kinds of hook, and only one of them is cross-platform.** The hooks page
describes **command-line hooks** (`--veloapp-install`, `--veloapp-obsolete`,
`--veloapp-updated`, `--veloapp-uninstall`, with 30 s / 15 s / 15 s / 30 s to
exit) and environment-variable hooks (`VELOPACK_FIRSTRUN`, `VELOPACK_RESTART`)
as cross-platform. It also says the convenience **fast callbacks** that wrap
them are Windows-only and are no-ops on macOS and Linux [doc:
https://docs.velopack.io/integrating/hooks]. The Rust crate agrees:
`on_after_install_fast_callback`, `on_after_update_fast_callback`,
`on_before_update_fast_callback` and `on_before_uninstall_fast_callback` are
`#[cfg(target_os = "windows")]` [doc:
https://docs.rs/crate/velopack/1.2.161/source/src/app.rs]. There is no
conflict between the page and the source; an earlier draft of this file said
there was, and it was wrong. Whether Velopack's macOS installer actually invokes
the command-line hooks is not stated beyond that sentence [not checked]. No hook
can show UI or cancel. **So the migration does not live in a hook** — it lives in
`prepare_data_dir`, as above.

**File associations and URL schemes are not Velopack's.** Issue #29, open
since February 2024: "Velopack does not yet support this"; the suggested
workaround is to write `HKCU\Software\Classes` in the after-install callback
and remove it in the before-uninstall one [doc:
https://github.com/velopack/velopack/issues/29]. That suits this plan:
`shell_handler.rs` already writes and undoes its own registration for the
portable copy [src: src-tauri/src/shell_handler.rs:1-60], so the beta reuses it
with its own names, from those two callbacks. On macOS the declarations come
from `Info.plist` (`--plist`) [inferred]. Shortcuts (Desktop, Start menu) are
Velopack's and are removed on uninstall [doc:
https://docs.velopack.io/integrating/shortcuts].

**Channels.** The default channel is the OS name; **each OS/arch needs its own
channel name** (e.g. `win-x64-beta`, `osx-arm64-beta`), the feed is
`releases.<channel>.json`, and a channel cannot be renamed, only copied [doc:
https://docs.velopack.io/packaging/channels]. Switching an installed copy's
channel is `UpdateOptions { ExplicitChannel, AllowVersionDowngrade }`, and the
app must apply such an update itself [doc:
https://docs.velopack.io/integrating/switching-channels]. This plan uses switching only for
later pre-releases of the stable package (see "Channels"); the beta and
stable are different package IDs, joined by a hand-off, not a channel change.

**Deltas.** Built when the previous release is in the output directory; CI
runs `vpk download` first; the client falls back to a full package after 10
deltas by default [doc: https://docs.velopack.io/packaging/deltas;
https://docs.rs/velopack/latest/velopack/struct.UpdateOptions.html].

**Hosting.** `vpk upload s3 --endpoint … --bucket … --prefix …
--keepMaxReleases N` for S3-compatible stores (the documented example is
Backblaze B2); R2 is not named [doc:
https://docs.velopack.io/distributing/deploy-cli] — expected to work as a
generic S3 endpoint [inferred], and version 1 already writes R2 with the AWS
CLI, which can stay. The client reads it with `HttpSource` [doc:
https://docs.rs/velopack/latest/velopack/sources/index.html].

**Signing.** Windows: `--signParams` (signtool), `--signTemplate` for other
tools, `--azureTrustedSignFile` without signtool "since 1.3", which is a
pre-release today [doc: https://docs.velopack.io/packaging/signing]. macOS:
`--signAppIdentity`, `--signInstallIdentity` (only for the `.pkg`),
`--signEntitlements` (pass ours — today's `entitlements.plist` exists for
libmpv's needs [src: src-tauri/tauri.macos.conf.json]), `--notaryProfile`,
`--keychain`; deep signing is the default and `--signDisableDeep` is for
pre-signed contents [doc: same page]. Notarisation with an API key directly is
an open issue (#762) [doc: https://github.com/velopack/velopack/issues/762],
hence V2-2's keychain profile built from the existing API key.

**Install location and uninstall.** Windows Setup: per user, one click, into
`%LocalAppData%\<packId>`, `--installto` on the command line; machine-wide only
through `--msi` [doc: https://docs.velopack.io/packaging/installer;
https://docs.velopack.io/packaging/operating-systems/windows]. **Uninstall
deletes the whole `%LocalAppData%\<packId>`** [doc:
https://docs.velopack.io/integrating/uninstalling]; data that must survive goes
elsewhere [doc: https://docs.velopack.io/integrating/preserved-files]. Hence
the beta's data folder is `app.frameplayer.beta`, never the `FramePlayerBeta`
install folder — and **the data directory is set explicitly**, because Skathi's
default `%LOCALAPPDATA%\<exe stem>\storage` [src: sk-storage/src/lib.rs:5-7]
would land inside the install folder if the exe were named after the package.
macOS: no uninstaller, the viewer trashes the app [doc: uninstall page].

**Living beside the NSIS installation.** Not documented; migration guides exist
only for ClickOnce and Squirrel [doc:
https://docs.velopack.io/category/migrating]. A different package ID gives a
different folder, uninstall entry and shortcuts, so no collision is expected
[inferred] — to verify in the hello-world.

**Open issues worth tracking** [doc:
https://github.com/velopack/velopack/issues?q=is%3Aissue+is%3Aopen+macos]:
#975 (macOS: the new bundle's signature is not checked before apply — our feed
signature plus a `codesign --verify` of the downloaded bundle closes it),
#1135 (path traversal in `download_updates` — another reason to verify the feed
before Velopack sees it), #1103 (a restart brings the app to the front),
#1102 (an interrupted delta falls back to the full download).

## Windows without a signature

Decision 4: no certificate for now. Version 1 is unsigned on Windows today
[src: .github/workflows/release.yml, no `signtool`], so this changes nothing for
the people who already run it. It does matter more for a beta, because the beta
asks them to run a new, unknown executable.

**What the viewer sees.**

1. The browser may warn that the file "isn't commonly downloaded". Edge, for
   example, then offers *Keep* and *Keep anyway* [not checked: the exact
   wording depends on the browser and version].
2. Running `Setup.exe` from Downloads shows SmartScreen's "Windows protected
   your PC". The viewer clicks *More info*, then *Run anyway* [inferred: the
   file carries the Mark of the Web, and reputation follows the hash and the
   publisher, src: docs/distribution.md:151-170].
3. **Smart App Control blocks it, with no override.** When the cloud service has
   no confident verdict, an unsigned app is blocked, and there is no per-app
   exception; the only way past it is to turn Smart App Control off [doc:
   https://support.microsoft.com/windows/smart-app-control-faq-285ea03d-fa88-4d56-882e-6698afdb7003].
   Version 1 has the same problem today [inferred].

**Does Velopack work unsigned?** Signing is something `vpk pack` does when it
is given `--signParams`, `--signTemplate` or `--azureTrustedSignFile`
[doc: https://docs.velopack.io/packaging/signing]. Nothing in the documentation
makes it a requirement [not checked beyond that page]. Integrity of an update
does not depend on Authenticode either: Velopack checks every package against
the feed's SHA-256, or its SHA-1 when the feed has no SHA-256, and the size
[doc: https://docs.rs/crate/velopack/1.2.161/source/src/manager.rs], and we sign
the feed itself with ed25519.

**What an unsigned build risks for auto-update.**

- **SmartScreen, probably not.** `Update.exe` downloads the packages itself,
  not through a browser, so the new files carry no Mark of the Web, and the
  SmartScreen prompt is a Mark-of-the-Web check [inferred; to confirm in step
  0 by looking for a `Zone.Identifier` stream after an update].
- **Smart App Control, yes, if it is on.** It judges every executable when it
  runs, not only downloaded ones, so an update could be blocked as well
  [inferred from the FAQ above]. It can only be on if it was on when the
  viewer installed the beta, which would have blocked the install anyway.
  Microsoft has since allowed turning it back on [doc: same FAQ].
- **Antivirus heuristics.** An unsigned program in `%LOCALAPPDATA%` that
  downloads executables and replaces itself is a pattern antivirus heuristics
  flag [not checked]. If it happens: file a false-positive report with the
  vendor, and publish SHA-256 sums so a viewer can check the file.

**What the download page says** (proposed, en; ru and zh follow the site's
dictionaries):

> **Windows will warn you.** The beta is not signed with a code-signing
> certificate yet, so Windows does not know who published it. If SmartScreen
> says "Windows protected your PC", click **More info**, then **Run anyway**.
> If Smart App Control is on, Windows will not let you install unsigned
> programs at all. You can wait for a signed release, or use the macOS
> version. The installer's SHA-256 is below; the source is on GitHub.

## Alternatives

- **Tauri's updater format with a separate endpoint.** `latest.json` with a
  minisign signature per platform, no channels — a channel is another endpoint
  [doc: https://v2.tauri.app/plugin/updater/]. Outside Tauri, the same model is
  `cargo-packager-updater` (with `cargo-packager` for dmg/NSIS/MSI) [doc:
  https://docs.rs/cargo-packager-updater/latest/cargo_packager_updater/;
  https://github.com/crabnebula-dev/cargo-packager]. It keeps today's key and
  knowledge but brings back NSIS, the in-place swap and its rules, and has no
  deltas — the ~1 900 lines V2-2 wants to retire [src:
  docs/ROADMAP-v2.md:50-57]. Rejected.
- **Sparkle + WinSparkle.** The most mature macOS updater: EdDSA-signed
  appcast, deltas, real channels (`sparkle:channel`), phased rollout [doc:
  https://sparkle-project.org/documentation/publishing/]; WinSparkle runs
  whatever installer we ship [doc: https://winsparkle.org/]. Two libraries
  behind Objective-C/C FFI (one community crate) and still no Windows installer
  of its own. Strong on macOS alone; a worse whole than Velopack.
- **cargo-dist.** CLI-oriented: shell/PowerShell/MSI/Homebrew installers, no
  `.app`/dmg yet, and its updater only updates what its shell installers put
  down [doc: https://axodotdev.github.io/cargo-dist/book/installers/index.html;
  https://axodotdev.github.io/cargo-dist/book/installers/updater.html]. Not a
  fit for a GUI app with dylibs.
- **Plain GitHub Releases with an in-app check.** No dependency, but signature
  checks, apply, restart and deltas are ours to write. Useful only as the beta's
  very first week if Velopack's hello-world fails; Velopack's `GithubSource`
  reads the same releases [doc: sources link above].

## Migration

### Version 2 reads version 1's data itself

Decision 2. There is no export from version 1. The importer reads:

- **macOS**: the WebKit SQLite store. The format is measured (above).
- **Windows**: WebView2's LevelDB. Chromium's key/value encoding is to be
  read from Chromium's source; the blocks are Snappy-compressed. **This remains
  the main risk of the migration** [not checked].
- **Files** (`mpv.conf`, fonts, the API key).

**Version 1 must not be running, and the importer opens only a copy.**

- Opening a WAL database, even read-only, can create the `-wal` and `-shm`
  files in the source directory, or fail when it cannot [doc:
  https://sqlite.org/wal.html#read_only_databases]. The importer must leave
  nothing behind in version 1's folders, including new files.
- A LevelDB directory copied while it is written can mix a `CURRENT`, a
  `MANIFEST` and table files from different moments [doc:
  https://github.com/google/leveldb/blob/main/doc/impl.md#files].
- So the importer first confirms version 1 is not running (bundle ID among
  running applications on macOS; on Windows, no `frameplayer.exe` running from a
  detected installation or portable folder [inferred]). It then copies the
  store's whole directory (the SQLite file **with** any `-wal`/`-shm` beside it,
  or the whole LevelDB folder) into the beta's own scratch space
  (`<v2 data>/import/<timestamp>/`), and confirms that version 1 did not start
  during the copy. Only after that does it open the copy, and only the copy. A
  LevelDB copy must also pass a consistency check: `CURRENT` names a `MANIFEST`
  that is present, and every table that manifest lists is present. If the check
  fails, the importer takes the copy again. Files are copied, never opened in
  place.
- While version 1 is running, the screen says so and waits ("Quit Frame Player
  to import"). It never ends version 1. Nothing is imported from a live store.

The presence of `%LOCALAPPDATA%\app.frameplayer\EBWebView` is **not** evidence
that version 1 is installed. Issue #81 shows a portable copy creating it even on
a machine with no installation [src: issue #81]. The evidence is a Local Storage
database containing `frameplayer.*` keys.

### Onboarding: finding version 1 and offering the import

**When.** On the first start of version 2, inside V2-9's welcome screen
[src: docs/ROADMAP-v2.md:330-356], as its first step when version 1 is found.
The detection itself runs in `prepare_data_dir`, before storage loads
[src: sk-app/src/embed.rs:378-381]. Because an error there stops startup, it
catches everything and turns failures into a line on the screen. The import is
**not** a Velopack hook: Velopack's install hooks exist only on Windows [doc:
https://docs.rs/crate/velopack/1.2.161/source/src/app.rs], and a copy dragged
out of a disk image, or a portable copy, never runs one.

**Detection.**

| Kind | How it is found | Source |
|---|---|---|
| Installed, Windows | the uninstall entry under `HKCU\…\Uninstall` whose `InstallLocation` holds `frameplayer.exe`, its `DisplayVersion`; then a Local Storage database with `frameplayer.*` keys under `%LOCALAPPDATA%\app.frameplayer\EBWebView` | [src: src-tauri/src/portable.rs:175-235 — version 1 reads the same entry] |
| Installed, macOS | Launch Services for `app.frameplayer`, its `CFBundleShortVersionString`; the WebKit store under `~/Library/WebKit/app.frameplayer` | [measured: path above] |
| Portable, Windows | the command of `FramePlayer.Magnet` or any `FramePlayer.<ext>` ProgId → the executable's folder → a `.portable` marker beside it → `.data/` | [src: .claude/rules/sources-and-privacy.md:45 — every copy writes the magnet candidacy; src-tauri/src/portable.rs:20-31] |
| Anything else | "Choose a folder…": a portable copy's folder, or a `.data` folder | — |

**What the screen shows.** A list of groups, each with its count and a
checkbox, all checked by default:

- Watch history: "N videos to continue, N positions". This group is absent
  when version 1's history was off.
- Track and picture choices: N.
- Hotkeys: N changed.
- Settings: N changed (only values that differ from the default, per V2-3).
- Subtitle fonts: N (size).
- Downloaded torrents: N (size), expandable to one row per torrent, plus
  "Remove them from the old version's folder", **on by default**
  (see "Torrents").
- OpenSubtitles account. On Windows it is imported. On macOS the line reads
  "you will be asked to sign in again" (see the table).

If version 1 is running, the whole import waits: "Quit Frame Player to
import", with a live state that turns into the Import button once the process
is gone. The importer never quits or kills version 1. Reading a live store is
not safe (see "Version 2 reads version 1's data itself"), and neither is
moving a live torrent.

**"Import later"** closes the step without importing. Settings → General →
"Import from Frame Player 1…" opens the same screen at any time. That entry
also runs a repeat import, as a **merge** (rules under "Merging two sources").

**Merging two sources.** The importer may run more than once, partially, or
from two sources (version 1, and at graduation the beta), so the merge rules
are explicit:

- **The destination's privacy settings win.** With history off in version 2,
  nothing history-shaped is imported from any source. Version 2's excluded
  folders filter every identified store, and so do the source's own.
- **Per entry, newest timestamp wins.** Positions, recent items and track
  choices carry `ts` [src: src/lib/history.svelte.ts:56,78,116]. A version 1
  position newer than the beta's is kept, so progress made in version 1 after an
  earlier import is not lost. An entry with no timestamp is taken only when the
  destination has none.
- **Deletions are remembered.** Version 2 keeps a tombstone for every entry the
  viewer forgets (`sourceId` and time) and a "history cleared at" time.
  `clearHistory` and `forgetRecent` write them. A merge never brings back an
  entry older than its tombstone or the clear time, so deleting something in
  version 2 is not undone by a later import from version 1. A tombstone is kept
  only as long as the entry it covers could still arrive, and it holds the id,
  not the title or path [inferred design].
- **Settings**: a source value is applied only where the destination still has
  the default (V2-3 knows which layer a value came from).
- **The import records what it took**: per source, the time of the last import
  and which groups. That is how graduation knows whether the beta's import was
  complete, selective or skipped (see "Graduation").

**One-way, and version 1 is never written.** The one exception is the torrent
folders the viewer moved. There is no continuous sync: two writers with
diverging schemas, and last-writer-wins storage underneath
[src: sk-storage/src/lib.rs:11-12]. **Switching back** means starting version 1.
Its state is as it was at the import, minus the moved torrents; progress made
in version 2 does not flow back, and the screen says so in one line.

**Proposed strings.** Keys under `onboard.import.*`. The zh column is a draft,
to be written against [zh-glossary.md](zh-glossary.md).

| Key | en | ru | zh (draft) |
|---|---|---|---|
| `title` | Bring over what you had | Перенесите то, что было | 导入原有内容 |
| `found` | Found Frame Player {version} on this computer. | На этом компьютере найден Frame Player {version}. | 在此电脑上找到 Frame Player {version}。 |
| `found_portable` | Found a portable Frame Player in {folder}. | Найдена портативная копия Frame Player в {folder}. | 在 {folder} 中找到便携版 Frame Player。 |
| `history` | Watch history: {videos} to continue, {positions} positions | История: продолжить — {videos}, позиций — {positions} | 观看记录：{videos} 个可继续观看，{positions} 个播放位置 |
| `tracks` | Track and picture choices: {n} | Выбор дорожек и картинки: {n} | 音轨与画面选择：{n} |
| `keys` | Hotkeys you changed: {n} | Изменённые горячие клавиши: {n} | 已修改的快捷键：{n} |
| `settings` | Settings you changed: {n} | Изменённые настройки: {n} | 已修改的设置：{n} |
| `fonts` | Subtitle fonts: {n} ({size}) | Шрифты субтитров: {n} ({size}) | 字幕字体：{n}（{size}） |
| `torrents` | Downloaded torrents: {n} ({size}) | Скачанные торренты: {n} ({size}) | 已下载的种子：{n}（{size}） |
| `torrents_move` | Remove them from the old version's folder | Убрать их из папки старой версии | 从旧版本的文件夹中移除 |
| `torrents_move_hint` | They are moved, not copied, so they take no extra space. The old version will no longer list them. | Они переносятся, а не копируются, поэтому место не удваивается. Старая версия перестанет их показывать. | 种子会被移动而非复制，不会额外占用空间。旧版本将不再显示它们。 |
| `running` | Quit Frame Player to import. | Закройте Frame Player, чтобы перенести данные. | 请退出 Frame Player 以导入数据。 |
| `account_relogin` | OpenSubtitles: you will be asked to sign in again. | OpenSubtitles: нужно будет войти снова. | OpenSubtitles：需要重新登录。 |
| `privacy` | Excluded folders stay excluded. Nothing leaves this computer. | Исключённые папки остаются исключёнными. Ничего не покидает этот компьютер. | 已排除的文件夹仍保持排除。所有数据都不会离开此电脑。 |
| `one_way` | What you watch here does not go back to the old version. | То, что вы посмотрите здесь, в старую версию не вернётся. | 在此观看的进度不会同步回旧版本。 |
| `import` | Import | Перенести | 导入 |
| `later` | Import later | Перенести позже | 稍后导入 |
| `choose` | Choose a folder… | Выбрать папку… | 选择文件夹… |
| `settings_entry` | Import from Frame Player 1… | Перенести из Frame Player 1… | 从 Frame Player 1 导入… |

### What migrates

| Item | v1 location (Win / macOS) | Format | v2 target | Notes |
|---|---|---|---|---|
| Watch positions, continue-watching | localStorage `positions`, `resume` | JSON in web store | Skathi localStorage, same keys | Filtered through `history.excluded` and `history.enabled` again on the way in; `resume` (the update snapshot) is **not** carried — it is a one-shot for v1's own relaunch [src: history.svelte.ts:41] |
| History prefs and privacy roots | `history` | JSON | V2-3 settings | Imported **first**, so the filter above uses them |
| Titles, recent links | `titles`, `links` | JSON | localStorage | Skipped entirely when `history.enabled` is false |
| Remembered tracks, folder tracks, picture, audio offsets | `tracks`, `tracks.folder`, `picture`, `audioOffsets` | JSON | localStorage | Keyed by `sourceId()`, which version 2 shares [src: .claude/rules/sources-and-privacy.md:46] |
| Downloaded subtitle list | `subs` | JSON (paths) | localStorage | Paths point next to videos, outside v1's folders, so they stay valid |
| Hotkeys | `keys` | JSON, overrides only | V2-3 settings | Drop overrides for actions version 2 no longer has; keep `e.code` values as they are |
| Playback and interface prefs | the "Playback prefs", "Interface" rows above | strings/JSON | V2-3 settings | Only non-default values (V2-3). `window` geometry: carried, but clamped to a visible screen and to v2's minimum size |
| Locale | `locale` | string | V2-3 settings | — |
| Catalog, indexer | `catalog`, `indexer`, `tmdb`, `tmdbCdn`, `release*` | JSON | V2-3 settings | `indexerApiKey` is a credential: carried, but stored where v2 keeps secrets if that differs |
| Casting | `castMode`, `castCacheCap`, `tv-hosts`, `airplay-*` | JSON | localStorage | `tv-devices` is a diagnostic trace, skip |
| Watch together | `relay`, `syncName` | strings | V2-3 settings | — |
| Torrent network prefs | `torrent` | JSON | V2-3 settings | — |
| Torrent list | `torrents` + `.meta` in `torrents/` | JSON, metadata bytes | v2's own session | Only the torrents moved (see "Torrents"); added from `.meta`, one re-hash on first open |
| Torrent content | v1's root (default cache `torrents/`) | folders `<Name> [<hash>]` | v2's root | **Moved** (renamed) when selected, never shared; see "Torrents" |
| `mpv.conf` | `%APPDATA%\app.frameplayer\` / `~/Library/Application Support/app.frameplayer/` | text | v2's config dir | Copied as a file; options version 2 does not honour are left in place and reported |
| Subtitle fonts | data `subtitle-fonts/` | font files | v2's data dir | Copied (small); APFS clone on macOS [inferred: `clonefile` costs no space] |
| OpenSubtitles API key | data `opensubtitles.key` | text | v2's data dir | Copied |
| OpenSubtitles login | Credential Manager / keychain `app.frameplayer.opensubtitles` | secret | v2's own entry | Windows: readable, copy [inferred: Credential Manager has no per-app ACL]. macOS: reading another app's item prompts the viewer [inferred: keychain ACLs name the creating app] — **ask once, or simply ask to sign in again** |
| Shell registration choice | `handler` (portable only) | string | — | Not migrated: the beta never registers by default |
| yt-dlp | data `bin/` | binary | — | Not migrated: re-downloaded on demand, as a fresh v1 does |
| Thumbnails, posters, cast, shaders, logs | cache dir | files | — | Not migrated: regenerable caches |
| `install-count` | config dir | word | — | Not migrated (see "Identity") |
| Unknown keys | `frameplayer.*` | — | — | **Not copied.** The importer works from an allowlist of keys **and fields**: each row above names what it takes, and anything else is dropped and counted (by key name, never by value) in the import log. A key version 1 adds later arrives only once the importer learns it. That is the price of the privacy promise: a new store of watched paths must not slip through unfiltered |

### Torrents

Version 1's torrent store, read from source:

- Three different things live under "the torrent directory". **`state`** (the
  app cache's `torrents/`) holds librqbit's session store, the `.meta`
  metadata cache and `.roots.json`. **`root`** is where new torrents go,
  `state` unless the viewer chose a folder. **`roots`** is every place data may
  already be [src: docs/rules/torrents-core.md:18;
  src-tauri/src/torrent/dirs.rs:48,55-100].
- Each torrent is a folder `<Name> [<infohash>]`, and the disk is the source
  of truth [src: docs/rules/torrents-core.md:19].
- **`is_ours` decides what version 1 may delete.** Inside `state` it is
  anything. In any other root it is only a direct child whose name parses back
  to an info hash. `list`, `forget` and `torrent_clear_cache` all go through it
  [src: src-tauri/src/torrent/dirs.rs:151-162; storage.rs:36-54].
- **At startup, `prune_orphaned_store` drops from librqbit's session store
  every torrent whose `output_folder` no longer exists**, together with its
  `<hash>.torrent` and `<hash>.bitv`, before the session restores anything
  [src: src-tauri/src/torrent/session.rs:180-182,480-552].
- A restore otherwise **recreates the folder**: `FileStorage::init` creates every
  file [src: session.rs:480-490]. That is how a deleted torrent used to come
  back as an empty copy of itself, and it is why the prune exists.
- **The remembered-torrent list drops entries whose data is gone, when it is
  read** [src: src/lib/torrent/remembered.ts:36-38].
- **The descriptor budget**: librqbit holds one descriptor per file of every
  torrent in the session. On macOS the soft limit is 256, so `torrent_storage.rs`
  opens files lazily within a process-wide budget [src:
  docs/rules/torrents-core.md:12].
- **Renaming under a live torrent**: on Windows it fails, because the files
  are open. On macOS it succeeds and leaves librqbit writing through handles to
  a path that no longer exists [src: docs/rules/torrents-core.md:19].

#### (a) One download folder shared by both versions

- **Both running, each with its own session.** Two librqbit sessions would
  write the same files, each with its own fastresume `.bitv`. A piece written by
  one is "missing" to the other, which fetches it again; that is harmless but
  wasteful [inferred]. **Deletion is not harmless.** "Delete" or "delete
  watched episodes" in version 1 removes a folder through `forget`, and a
  version 1 cache clear removes every recognised folder in `state`
  [src: docs/rules/torrents-core.md:18]. On macOS the unlink succeeds while
  version 2's session holds the files, so version 2 goes on writing into
  unlinked inodes and loses the data silently. On Windows the delete fails or
  stays pending [inferred]. The next restore of the deleted torrent in version 2
  recreates it as an empty copy [src: session.rs:480-490].
- **Both own the session/resume state.** Each keeps its own `session.json` in
  its own `state`, so neither knows the other changed the disk. Version 1's
  prune catches only a *missing folder*, not changed contents [src:
  session.rs:505-532].
- **Can version 2 own the files while version 1 only loses its entries?** Not
  with the folder shared. Version 1 lists every recognised folder in every root
  straight from disk [src: storage.rs:39-54], so it cannot lose the entries
  while the folders stay in its roots. It also has no "forget but keep files"
  operation for another program's data. Making it safe needs a version 1 change
  (a "foreign" marker that `is_ours` refuses), and every version 1 copy older
  than that change stays dangerous.
- **What version 1's cleanup deletes**: anything inside `state`, and in a
  chosen root only the hash-named folders [src: dirs.rs:151-162]. A shared root
  is therefore exactly the set version 1 is allowed to delete.

**Verdict: no.** It needs a version 1 change to be safe, and old version 1 copies
would never be safe.

#### (b) Onboarding moves the selected torrents (recommended)

- **The unit is a whole torrent folder**, never single files. Moving the
  folder is what makes version 1's next start drop the torrent cleanly through
  the prune. A folder left behind with some files missing would instead be
  restored, and the missing files recreated empty [src: session.rs:480-490,
  505-532].
- **Where they go, chosen so that the move is a rename.**
  - If the torrent is in version 1's `state` (the default), it is renamed into
    version 2's own torrent root. That root is version 2's cache directory, on
    the same volume as version 1's (both under `~/Library/Caches` or
    `%LOCALAPPDATA%`) [inferred].
  - If the torrent is in a root the viewer chose, it is renamed into a
    `Frame Player 2` folder inside that same root, and that folder becomes a
    version 2 root. The torrent stays on the drive the viewer picked, and the
    move is still a rename. Stable version 2 keeps that folder as a root after
    graduation; nothing is moved back (decision 7).
  - **The destination is validated against version 1's own deletion rule.**
    `is_ours` accepts **anything** directly under `state`, and in any other root
    a direct child whose name parses to an info hash. Roots are compared
    canonically [src: src-tauri/src/torrent/dirs.rs:151-162]. So the move
    reads version 1's `.roots.json` (`root` and every `seen` entry) and its
    `state` [src: dirs.rs:48,75-100], canonicalises all of them, and refuses a
    destination `D` (the `Frame Player 2` folder or version 2's own root) when:
    - `D` is a version 1 root, or inside one at any depth other than the
      `Frame Player 2` child of a chosen root;
    - `D`'s parent is version 1's `state` (anything directly under `state` is
      deletable);
    - `D` is or contains version 1's `state`;
    - `D`'s name parses to an info hash.

    Only then is the claim true that version 1 never lists or deletes it: its
    name parses to no hash, so `list` and `forget` skip it, and the torrents
    inside it have a parent that is no version 1 root [src: dirs.rs:151-162;
    storage.rs:39-54]. A path that cannot be canonicalised is refused, the
    rule version 1 itself uses [src: dirs.rs:164-175]. If no destination on the
    same volume passes, the torrent is not moved, and the row says why.
  - Same-volume is checked by device ID (macOS) or volume serial number
    (Windows) before renaming [inferred]. A rename is instant and needs no
    space.
- **Copy-then-delete only across volumes**, which happens only when the viewer
  picks another drive. It starts with a space check against the **allocated**
  size. That is blocks on macOS, where librqbit's files are sparse; on Windows
  it is `len()` unless the file is sparse, and version 1 now marks files sparse
  [src: docs/rules/torrents-core.md:19; torrent_storage.rs:36-40]. The copy
  keeps the holes (`SEEK_HOLE`/`SEEK_DATA` on macOS, allocated ranges on
  Windows) [inferred]. **The source is kept until the destination is verified
  and registered**: every file present with the same length, the downloaded
  pieces re-hashed against the torrent's metadata, the metadata copied, and the
  torrent added to version 2's session and its store written. Only then is the
  source deleted, and only after re-checking that version 1 is not running. If
  version 1 started in between, the source stays and the deletion waits for
  the next start of version 2.
- **The metadata comes along as a copy.** Version 2 adds each torrent from
  version 1's cached `.meta` bytes, so there is no network lookup. It does not
  take over version 1's `.bitv`, which is tied to version 1's session and path.
  The cost is one re-hash on the first open: about 3.2 s for a 7.5 GB season
  [src: docs/rules/torrents-core.md:19].
- **The remembered magnets, positions and catalog links** come through the
  localStorage import. Positions are keyed by `sourceId`, so "continue
  watching" finds the moved files [src: .claude/rules/sources-and-privacy.md:46].
- **With "Remove them from the old version's folder" off**, the torrent is
  copied (an APFS clone on macOS, at no space cost [inferred]), or left in place
  and re-fetched by version 2. Version 1 keeps its copy.
- **If version 1 is running, the move is refused and waits.** On macOS a
  rename under a live torrent leaves librqbit writing through stale handles; on
  Windows it fails [src: docs/rules/torrents-core.md:19]. Version 1 is
  detected by bundle ID among running applications (macOS), or by a running
  `frameplayer.exe` from a detected installation or portable folder (Windows)
  [inferred]. The screen waits for it to exit and never ends it.
- **Version 1 starting during the move.** Version 1 honours no lock or marker
  of ours, and copies already installed never will. So the move cannot be made
  airtight from outside. It narrows the window instead:
  - the process check runs again **immediately before each rename**, and on
    macOS the move also watches for `app.frameplayer` launching
    (`NSWorkspace` notification) and stops between two folders;
  - each folder is one rename, so each torrent is either moved or not.

  **The residual risk, stated plainly:** version 1 can start in the
  milliseconds between a check and a rename. Its startup prunes the session
  store *before* it restores torrents [src: session.rs:180-182]. A folder
  renamed after the prune but before the restore is recreated by the restore
  as an empty copy in version 1's folder [src: session.rs:480-490]. The
  viewer's data is already in version 2's folder, so nothing is lost, but
  version 1 shows a 0 MB row until its next start prunes it. On Windows a
  rename that meets an open file fails, and the journal retries it later.
  Restored torrents are paused and their files are opened only on a read
  [src: session.rs:198,317; torrent_storage.rs:21-26], so version 1 holds no
  handle to a folder nobody is playing [inferred].
- **The move is journalled before either side is touched.** The journal
  `<v2 data>/import/torrent-moves.json` is written with a temporary file and a
  rename, like librqbit's store [src: session.rs:537-545]. It holds one record
  per torrent: info hash, source path, destination path, method (rename or
  copy), and state: `planned` → `renamed` | `copying` → `copied` → `verified` →
  `registered` → `source-removed` → `done`. The record is written *before* each
  step and updated after it. At every start, version 2 reads the journal and
  finishes or rolls back each unfinished record:
  - `planned`, with the source still in place: nothing happened yet; drop the
    record.
  - `renamed`, or a rename whose destination exists and source is gone: go on to
    registration.
  - `copying`: delete the partial destination and copy again. The source was
    never touched.
  - `copied` or `verified`, but not registered: verify again, then register.
  - `registered`: remove the source, after the process check.

  A rollback is offered on the onboarding screen for records that cannot go
  forward (destination missing, source present). It renames back or deletes
  the partial copy. Nothing on the source side is removed until the record says
  `registered`.
- **Consent.** "Remove them from the old version's folder" stays on by default
  (decision 3). The onboarding checkbox, with its hint and the per-torrent
  rows, is the viewer's explicit consent. The move runs only after "Import" is
  pressed.

**What version 1 does on its next start** [src]: the prune removes the moved
torrents from the session store, along with their `.torrent`/`.bitv`
(session.rs:505-552). `list` no longer finds the folders (storage.rs:39-54). The
remembered-torrent list drops them when it is read (remembered.ts:36-38). It
does not crash and does not re-download. A "continue watching" card for an
episode still exists, keyed by its source. Opening it re-adds the torrent from
the remembered magnet, if that is still kept, and **downloads again into version
1's folder**, as any re-opened torrent does [inferred from the same files].
**Version 1 needs no change to be correct.** Two optional improvements are
items 6 and 7 of [v1-backports.md](v1-backports.md).

### Privacy

From [docs/rules/sources-and-privacy.md](rules/sources-and-privacy.md) and its
short form:

- The importer is local: nothing it reads leaves the machine; no request is
  made on its behalf.
- `history.enabled = false` means positions, titles, links, `subs` and the
  folder track memory are not imported at all; excluded folders filter every
  identified store, using the same `sourceId`/`pathUnder` test version 1 uses
  [src: .claude/rules/sources-and-privacy.md:46,50].
- Thumbnail storyboards are not migrated, so a private folder cannot leak one
  through the copy [src: history.svelte.ts:1-9 — privacy is enforced in Rust
  for thumbnails].
- The issue report's redaction (the ninth enforcement point) must learn the
  beta's folder names; the import's own log lines carry counts, never paths
  [src: .claude/rules/sources-and-privacy.md:49].
- If the beta turns on Skathi's at-rest encryption, the importer writes
  through Skathi's storage, not into `localStorage.json` by hand, or the first
  boot seals plaintext the import left behind and the two disagree
  [src: sk-storage/src/lib.rs:5-13; sk-app/src/embed.rs:360-367].

## The "Version 2 beta" button in version 1

### How version 1 learns the beta exists

Not from `latest.json`: the Tauri updater hands the app a manifest only when it
announces a newer version [inferred from src: src/lib/updater.svelte.ts:86-96,
where `check()` resolves to `null` otherwise], and on Windows the announcement
comes from `update_check` [src: src-tauri/src/update.rs:190]. So a separate,
tiny file, `<v2-updates-host>/announce/v1-beta-offer.json`, fetched by a Rust
command once per start (and at most once a day), no cookies, no identifiers:

```json
{
  "enabled": true,
  "version": "2.0.0-beta.4",
  "minV1": "1.52.0",
  "platforms": {
    "darwin-aarch64": { "minOs": "15.0", "download": "https://<v2-updates-host>/beta/osx-arm64/Frame%20Player%20Beta.dmg", "size": 123456789 },
    "windows-x86_64": { "minOs": "10.0.19041", "download": "https://<v2-updates-host>/beta/win-x64/FramePlayerBeta-win-Setup.exe", "portable": "https://<v2-updates-host>/beta/win-x64/FramePlayerBeta-win-Portable.zip", "size": 98765432 }
  },
  "page": "https://<site>/beta",
  "notes": { "en": "…", "ru": "…", "zh": "…" }
}
```

- `enabled: false`, a missing file, a network error or a platform absent from
  `platforms` all mean **no button** — the remote off switch.
- `minV1` lets the offer require a version 1 that has some fix the import
  depends on.
- **The offer is signed, and its links are checked.** The Windows installer is
  not Authenticode-signed (decision 4), so "the installer carries its own
  signature" would be false on Windows. Hence:
  - `v1-beta-offer.json` is signed with the v2 feed key (`.sig` beside it), and
    version 1 carries that public key and refuses an offer that does not verify;
  - every `download` and `portable` entry carries its **SHA-256** and size
    inside the signed file;
  - version 1 accepts only `https://` links to an allow-listed host, which is
    `<v2-updates-host>` alone (a constant in version 1, not a field of the
    file), so a valid signature cannot point at someone else's server;
  - version 1 still opens the page or the download in the browser and runs
    nothing itself. The checksum is printed on `<site>/beta` and in the modal,
    so the viewer can compare it [inferred: a browser download cannot be
    verified by version 1].
- **What leaves the machine**: one GET to the project's own host, the same kind
  of request as the update check. The settings sheet's "what leaves this
  machine" wording (V2-9 step 5) and the privacy rules should list it. A
  switch "Offer the version 2 beta" in settings turns the request off.

### Platform detection

Rust knows the OS and the architecture at compile time; the key is the same
one the update manifest uses (`darwin-aarch64`, `windows-x86_64`,
`windows-aarch64`). An x64 copy on an ARM64 machine asks for `windows-x86_64`,
like its updates [src: .claude/rules/build-and-release.md, architecture line].
`minOs` is checked against the running OS. A **portable** copy
(`portable::is_portable()`) gets the portable zip as the primary link. The
button is also hidden when the beta is already installed (Windows: its
uninstall entry; macOS: Launch Services knows `app.frameplayer.beta`)
[inferred], and after "Not now" until the offered version changes.

### UX

- Placement: in the title bar beside the update button
  [src: src/lib/components/TopBar.svelte:191-205], same height, quieter
  styling; the update button wins when both would show and space is short.
- Click → a modal: one heading, three short points about what is new, a
  sentence that version 1 stays installed and untouched, a sentence on what is
  imported and that progress made in the beta does not come back, the download
  button (with size), "Open the beta page", "Not now".
- The download opens in the browser (`plugin-opener`), never a silent download
  and run: the viewer sees a signed installer from the project's site
  [inferred: avoids an installer launched from an unexpected place].

### Proposed strings

Proposed wording only; the keys follow the `bar.*` pattern, and the Chinese is
to be written against [zh-glossary.md](zh-glossary.md), so it is marked as a draft.

| Key | en | ru | zh (draft) |
|---|---|---|---|
| `bar.beta` | Version 2 beta | Бета версии 2 | 2.0 测试版 |
| `beta.title` | Try Frame Player 2 | Попробуйте Frame Player 2 | 试用 Frame Player 2 |
| `beta.lead` | A new interface, drawn natively, with the video inside it. It installs beside this version and does not change it. | Новый интерфейс, нарисованный нативно, с видео внутри него. Ставится рядом с этой версией и ничего в ней не меняет. | 全新的原生界面，视频融入其中。与当前版本并存安装，不会改动当前版本。 |
| `beta.import` | Your history, settings and hotkeys are copied on first start. What you watch in the beta stays in the beta. | История, настройки и горячие клавиши копируются при первом запуске. То, что вы посмотрите в бете, остаётся в бете. | 首次启动时会复制您的观看记录、设置和快捷键。在测试版中的观看进度不会同步回来。 |
| `beta.download` | Download ({size}) | Скачать ({size}) | 下载（{size}） |
| `beta.portable` | Portable version | Портативная версия | 便携版 |
| `beta.page` | What's new | Что нового | 新功能 |
| `beta.later` | Not now | Не сейчас | 以后再说 |
| `settings.beta_offer` | Offer the version 2 beta | Предлагать бету версии 2 | 提示 2.0 测试版 |

## Graduation: version 2 replaces version 1, and the beta retires

**Version 1 viewers.** V2-2's bridge [src: docs/ROADMAP-v2.md:122-133], made
into one staged, restartable cutover. Stable's onboarding finds version 1's data
folder **as its own** and converts the web store into Skathi's file, leaving the
old store where it is.

### The cutover (Windows)

Why the naive order is unsafe, from source:

- Version 1's uninstaller runs `NSIS_HOOK_PREUNINSTALL`. It deletes
  `$INSTDIR\.portable` unconditionally. Unless the uninstaller runs in update
  mode, it runs the player with `--unregister-shell`. When the uninstaller's
  "delete app data" checkbox is set, it removes `$INSTDIR\.data` recursively
  [src: src-tauri/installer-hooks.nsh:129-151]. That checkbox's value in a
  silent run is not known [not checked: Tauri's NSIS template is not in this
  repository].
- NSIS then deletes its own file list, fixed when it was installed, from
  `$INSTDIR` and removes the folder [src: installer-hooks.nsh:130-132 — the
  template's `RMDir "$INSTDIR"`; docs/rules/build-and-release.md, the in-place
  update line — "the NSIS uninstaller deletes a list fixed when it was
  written"]. A
  version 2 installed into the same folder **first** can lose files whose
  names are on that list.
- `--installto` over an existing directory lets Velopack replace what is there
  [doc: https://docs.velopack.io/packaging/installer; the behaviour on a
  non-empty folder is not documented, not checked].

So the bridge (the last version 1 release) runs these stages. Each is recorded
in a journal before it starts (`%LOCALAPPDATA%\FramePlayer.cutover\journal.json`,
written by temporary file and rename), and is re-entered from the journal if
the process dies. The bridge is the version 1 player itself, so it runs at the
next start after an interruption.

1. **Preflight, nothing changed yet.** Classify the copy: an ordinary
   installation in the default folder, one in a chosen folder, an installation
   made portable by the installer checkbox (`.portable` with an uninstall
   entry), or a portable archive (no uninstall entry) [src:
   src-tauri/src/portable.rs:175-235]. Look for an existing `FramePlayer`
   installation (the beta's hand-off may already have put one there): if
   present, skip to stage 5 and adopt it. Choose the **target**:
   - default folder: Velopack's own `%LOCALAPPDATA%\FramePlayer`, a different
     folder from NSIS's `%LOCALAPPDATA%\Frame Player`, so no directory is
     reused;
   - chosen folder: a sibling `<parent>\FramePlayer`, same drive and same
     parent, again not the NSIS folder;
   - portable (either kind): the copy's own folder, the one case where reuse
     is the point.

   Validate the target: on a fixed local volume, writable without elevation,
   enough free space for the full package, and either absent or empty (or the
   portable copy's own folder). Download the stable `Setup.exe`, or the
   portable zip, and version 1's own NSIS installer as the rollback. Verify both
   against the signed SHA-256 (finding 3). Refuse, and keep version 1 running
   as it is, if any check fails.
2. **Put user files out of reach.** For a portable copy, rename `.data`
   (and `.portable`) into `%LOCALAPPDATA%\FramePlayer.cutover\staging\` when
   that is on the same volume, or into `<folder>.cutover\` beside the copy
   otherwise. A rename never copies [inferred: same volume]. For every kind,
   record any file in the installation folder that is not on version 1's
   payload list as a **user file**, and move it the same way [inferred: the
   payload list is what version 1's in-place update already compares against;
   its own rule for top-level names it does not own is `is_foreign_top_level`,
   src: src-tauri/src/update.rs:443]. Nothing that is the viewer's own is
   left where an uninstaller can reach it.
3. **Retire NSIS** (installations only, not portable archives). Run the
   uninstaller silently in the template's update mode, so that
   `--unregister-shell` is skipped and the file types keep opening something
   [src: installer-hooks.nsh:141-147] [not checked: that the uninstaller
   accepts the template's update flag when run directly; to verify in step
   0]. Then confirm that its uninstall entry is gone and its file list is
   removed. If update mode is not available, the shell registration is
   briefly gone, and stage 5 restores it.
4. **Install version 2** into the validated target: `Setup.exe --silent
   --installto <target>` for an installation, the portable zip unpacked for a
   portable copy. Move the staged `.data` and `.portable` back, and the user
   files.
5. **Confirm, then move associations.** Start version 2. It writes a
   `cutover-ok` mark into the journal once it has opened its data and finished
   onboarding's conversion. **Only after that mark** does version 2 take over
   the shell registration (`FramePlayer.<ext>`, `FramePlayer.Magnet`, the
   `frameplayer` scheme, Default apps), through the same `shell_handler` code
   it inherits [src: src-tauri/src/shell_handler.rs]. Then the journal and the
   staging folder are removed.
6. **Rollback**, offered when stage 4 or 5 fails: reinstall version 1 from the
   verified installer kept in stage 1, into its original folder, and move the
   staged files back. The data was never deleted, so version 1 finds
   everything.

**macOS** is simpler, because the bundle ID stays and the data folder is the
same. The bridge downloads stable's `.app` archive and verifies it (SHA-256
from the signed metadata, then `codesign --verify --deep --strict` and the
notarisation ticket). It renames `Frame Player.app` to
`Frame Player.app.v1-old` in the same folder, moves the new bundle in, and
starts it. It trashes the old bundle only after version 2 writes its
`cutover-ok` mark. If version 2 fails to start, it renames the old one back.
There is no portable copy on macOS [src: src-tauri/src/portable.rs:39-41].

### Beta viewers: the hand-off, with no package built for it

1. Every beta release, from the first, reads a signed
   `<v2-updates-host>/announce/beta-status.json` with its update check:
   `{"state": "active" | "ended", "stable": {<platform>: {version, url, sha256, size}}}`,
   signed with the feed key and verified before anything in it is used. The
   hand-off code is therefore already installed when it is needed. A beta older
   than that code would keep working unchanged, which is why it goes into
   beta.1.
2. At graduation CI writes `state: "ended"`. The beta's next start shows **"This
   beta has ended"**, one paragraph, and one button: **"Install Frame Player 2"**.
3. The button downloads stable's installer from the URL in the status file,
   **only from the allow-listed host** (`<v2-updates-host>`, a constant in the
   beta), into the beta's own folder. It then checks the downloaded bytes
   against the signed `sha256` and `size`, **before anything runs**. On Windows
   Setup is unsigned, so this check is the only integrity there is. On macOS
   the disk image is also checked with `codesign --verify` and `spctl` before it
   is opened for the usual drag. A mismatch deletes the file and says so. The
   hand-off does not use the beta's own `UpdateManager`, because stable is
   another package.
   - **The order relative to version 1's bridge matters, so it is defined.**
     Both the hand-off and the bridge first look for an existing `FramePlayer`
     installation: its uninstall entry on Windows, `app.frameplayer` at version
     2 or later on macOS. If one exists, they **adopt it** (launch it and let
     its onboarding import) instead of installing again. Whichever runs second
     therefore finds the first one's install, wherever the bridge put it
     (including a viewer's chosen folder, see "The cutover"). That is what
     makes them converge. The bare claim "same package ID, same folder" held
     only for the default path.
4. **Stable imports the beta's data with the same importer** that reads
   version 1. That is one shared importer with two sources:
   - version 1's web store, for v1 data;
   - the beta's own `localStorage.json` and V2-3 settings file, a direct read
     (the beta already uses version 2's formats).

   When both exist, onboarding imports **both**, by the rules under "Merging two
   sources". It does not assume the beta's import was complete: that import
   may have been skipped, selective ("Import later", groups unticked) or older
   than progress made in version 1 since. The beta's import record says which
   groups it took and when. Per entry the newest timestamp wins, the beta's
   tombstones and "cleared at" time stop version 1 from bringing back what was
   deleted in the beta, and the destination's privacy settings filter both
   sources. Torrent folders the beta owns are renamed into stable's root, by
   the same rules and the same journal as the move above.
5. **The beta retires itself through stable's onboarding**, with one checkbox,
   "Remove Frame Player Beta", on by default once the import has succeeded:
   - **Windows**: run the beta's own `UninstallString` from its `HKCU` uninstall
     entry, the entry Velopack registered [doc:
     https://docs.velopack.io/integrating/uninstalling]. This is more reliable
     than calling an `Update.exe` subcommand: Velopack's CLI reference lists an
     uninstall command, but its integration page describes uninstalling only
     through Apps & features [doc:
     https://docs.velopack.io/reference/cli/content/update-windows], so the
     registered command is the one that is certain to match the installed
     version. The beta's uninstall hook removes its `FramePlayerBeta.*` keys.
   - **macOS**: move `Frame Player Beta.app` to the Trash (`trashItemAtURL`).
     Velopack has no macOS uninstaller [doc: same uninstall page].
   - **The beta's data folder (`app.frameplayer.beta`) is kept**, like version 1's.
     Settings offer "Remove old beta data", which deletes it on request.
6. A beta viewer who ignores the screen keeps a working beta. The beta feed
   stays online, frozen at the last beta. Nothing is killed remotely.

**Later pre-releases** are the `beta` channel of `FramePlayer`, opted into in
settings (see "Channels"). There is no second application again.

## How others did it

- **1Password 7 → 8**, a rewrite: separate app and data; the version 8
  installer moved version 7 to the Trash and left a zipped backup; staff
  advised against running both because both talked to the browser extension
  [doc: https://1password.community/discussion/129914/1password-8-automatically-moves-1password-7-to-the-trash;
  https://1password.community/discussion/127283/can-i-have-both-1password-7-and-8-installed-at-the-same-time].
  Lesson: never touch version 1 automatically during the beta; the shared
  resource that would clash here is the default-player registration.
- **Firefox** since 67: one profile per installation, so Nightly, Beta and
  Release run side by side; data went to the first install opened after the
  change; downgrading can damage a profile [doc:
  https://blog.nightly.mozilla.org/2019/01/14/moving-to-a-profile-per-install-architecture/;
  https://support.mozilla.org/kb/understanding-depth-profile-installation].
  Lesson: separate data and a one-way import, never shared writes.
- **VS Code Insiders** "runs next to the Stable build" [doc:
  https://code.visualstudio.com/insiders/]; **Discord PTB/Canary** are separate
  apps [doc: https://support.discord.com/hc/en-us/articles/360035675191]. Their
  separate data folders are not stated on official pages [not checked]. Their
  shape is a permanent preview edition; this plan deliberately does **not**
  copy that (decision 1) and keeps only the separate identity during the beta.

## Risks

1. **Reading WebView2's LevelDB on Windows** remains the main unknown of the
   migration, and there is no export to fall back on. Mitigation: it is step 1
   of the work, against copies of real profiles. If it fails, the Windows beta
   ships anyway, importing files and torrents only; history and settings follow
   in a later beta update (decision 6). Until then the onboarding screen lists
   those two groups as "not yet available on Windows" rather than hiding them.
2. **Unsigned on Windows.** SmartScreen asks once per download. Smart App
   Control blocks it with no override [doc: Smart App Control FAQ above].
   Antivirus false positives are possible.
3. **Velopack on macOS.** An update of a bundle in `/Applications` from a
   standard account has not been tried (V2-2's open question). Issue #975: the
   new bundle's signature is not checked before apply. Notarisation works only
   through a keychain profile (#762).
4. **Two players fighting over the system**: associations, `magnet:`,
   `frameplayer://`, single instance, the keychain. Mitigated by the identity
   table; verified with both installed.
5. **Leaking into version 1's folders**, the shape of issue #81. Mitigation: one
   path function, and the hash check in "Order of work". The torrent move is the
   one write the check expects.
6. **The torrent move.** It is the only step that changes version 1's disk, and
   it runs over tens of gigabytes. Mitigations: whole folders only, rename
   only on the same volume, a destination validated against version 1's
   `is_ours`, a journal written before each step, refuse while version 1 runs
   (with the residual start-up race stated under "Torrents"), and delete across
   volumes only after the copy is verified and registered. Each rule is to be tested against the
   `a_chosen_root_protects_what_it_did_not_create` pattern
   [src: docs/rules/torrents-core.md:18].
7. **The hand-off depends on code shipped in beta.1.** A beta installed before
   that code exists cannot be retired from the server. Mitigation: no public
   beta before the status-file reader is in.
8. **The feed key** could end up existing only in GitHub, like version 1's.
   Mitigation: an offline backup from day one.
9. **Branch drift.** Merging `main` into `v2` after every v1 release keeps
   conflicts small; skipping merges makes graduation one large merge.
10. **Velopack's bus factor**: one principal maintainer. The feed is static
   files, so leaving Velopack later means replacing a client, not the hosting.

## Order of work

Rough effort for one person who knows the code.

| # | Step | Effort | Gate |
|---|---|---|---|
| 0 | **Velopack hello-world of the Skathi app.** macOS: signed, notarised, dmg, then **two** updates (A → B → C) from a static R2 prefix, a standard account, `/Applications`: the first update after a dmg install is a full download, because only Windows installs ship a local base package [doc: https://docs.velopack.io/integrating/overview#delta-fallback-rules], so deltas are judged on the second. Windows, **unsigned**: Setup through SmartScreen, a delta update with no prompt (check for a `Zone.Identifier` on the updated files), uninstall leaving the data folder, all beside an NSIS-installed version 1. Also: whether the NSIS uninstaller accepts the template's update flag when run directly (for "The cutover") | 2–3 days | Go/no-go for Velopack |
| 1 | **Readers for version 1's stores**: macOS SQLite (small); Windows LevelDB against copies of real profiles, taken with version 1 running and not | 2–4 days | Not a release gate: without it Windows imports files and torrents only (decision 6) |
| 2 | Identity: bundle ID, package ID, data dir set explicitly, single-instance key, keychain service, shell registration under the beta's names from the Windows hooks | 2–3 days | Checklist run with both installed |
| 3 | Onboarding import: detection, the screen, the groups, privacy filtering, merge on repeat, the settings entry | 4–6 days | Version 1's folders unchanged before/after (hash) |
| 4 | Torrent move: whole folders, same-volume rename, cross-volume copy with a space check, wait for version 1 to exit | 3–4 days | Version 1 starts cleanly after a move, and nothing outside the moved folders changed |
| 5 | The `v2` branch's `release-v2.yml`, `<v2-bucket>`, feed signing and verification, `beta-status.json` reader, retention, pre-releases on GitHub | 3–4 days | Windows: beta.1 → beta.2 by delta. macOS: beta.1 → beta.2 full (expected, see step 0), then beta.2 → beta.3 by delta |
| 6 | `/beta` on `main` in three languages, with the unsigned-Windows text and SHA-256 | 1–2 days | — |
| 7 | **Later**: version 1's button, `v1-beta-offer.json`, the settings switch and the strings, shipped in a normal version 1 release with `enabled: false` and switched on by the manifest | 2 days | Beta judged stable enough |
| 8 | Graduation: stable package, the beta's "ended" screen through the status file, stable's import from the beta, the version 1 bridge (V2-2) | separate plan | — |

**Verification for every step.** List and hash every file, recursively, under
`%APPDATA%\app.frameplayer`, `%LOCALAPPDATA%\app.frameplayer` and
`%LOCALAPPDATA%\Frame Player`, and under every **portable profile** used in the
test (a portable copy's folder with its `.data`). On macOS, do the same for
`~/Library/{Application Support,Caches,WebKit}/app.frameplayer`. Do it before
and after installing, running, importing, updating and uninstalling the beta,
and compare the two **file lists** as well as the hashes. No file may be
changed, **created** (for example an SQLite `-wal`/`-shm` beside the WebKit
store, or a LevelDB `LOG`) or deleted. The one exception is the torrent folders
a move took away, as recorded in the move journal. Issue #81 shows this test was
missing; it showed changed files, and the test must also catch new ones.

## Open questions

None at the moment; the answers so far are under "Decided".

## Appendix: version 1 changes

The two optional version 1 torrent changes that were listed here have moved
to [v1-backports.md](v1-backports.md), items 6 and 7, beside the other
improvements version 2's work found for version 1. The measurements behind
version 2 itself are in [native-frames.md](native-frames.md).

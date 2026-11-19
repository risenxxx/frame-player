# Roadmap: version 2

What version 2 is, what has been decided for it, and the work it is meant to
carry. [ROADMAP.md](ROADMAP.md) remains the roadmap of the player that ships
today. This file covers the move to a native interface and the features that
only make sense once that move is made.

Items are numbered **V2-n**. The numbers are stable for the same reason as in
`ROADMAP.md`: comments and documents refer to them. Each item says where it
stands:

- **decided**: settled, and the work starts from it;
- **planned**: wanted, with the shape known;
- **to measure**: depends on a number nobody has yet;
- **considered**: an idea, not a commitment.

The measure is the same as for version 1: **value ÷ effort against what a
viewer actually does**. A theme or a glass panel earns its place by being what
a viewer looks at for hours, not by being on another player's feature list.

## What version 2 is

The interface moves from a transparent web view over a native video window
([architecture.md](architecture.md)) to [Skathi](https://github.com/argon-chat/skathi).
Skathi draws the interface itself from TypeScript, HTML and CSS, and takes the
video as a GPU texture through its `<sk-surface>` element.

The video then stops being a hole in the page and becomes part of the scene.
Several limits of version 1 go away with that, including the one behind V2-5:
in version 1 the interface cannot blur or refract the video, because the web
view has nothing underneath it to read (see the comment on `.osd` in
`Osd.svelte`).

## Decided

### V2-1. Skathi, with mpv rendering into a shared texture (decided)

The renderer is gpu-next through the render API on both platforms: D3D11 on
Windows, and a Vulkan render API of our own on macOS. The reasons, the
measurements and the synchronisation rules are in
[native-frames.md](native-frames.md), "Decided for version 2". This file does
not repeat them.

### V2-2. Installing and updating through Velopack (decided)

Velopack replaces the Tauri updater and the NSIS installer. Skathi leaves this
part to the application ("your exe is yours to package, sign and update", its
embedding guide), so leaving Tauri means rebuilding it in any case. V2-9
depends on Velopack only for a first-run signal, and does not rely even on that
(see there). Today's chain is in [distribution.md](distribution.md). What
follows was read in Velopack's source (November 2026) unless it says measured.

**What it retires.** `update.rs`, `pack-windows-update.ps1` and
`installer-hooks.nsh`, about 1 900 lines. With them go the rules that exist only
because of the in-place swap: the `*.fp-old` rename, "an update may not add a
path", and the NSIS fallback whose code never returns. Velopack replaces the
installation's `current/` directory as a whole and deals with locked files
itself. `webview2.rs` goes with Tauri, since Skathi draws with its own
renderer.

**What it gives.** Delta packages (up to 10 deltas by default before it falls
back to a full package), channels, an update downloaded in the background and
applied on exit or on "Restart", and a portable zip, built by the same
`vpk pack`, that updates itself. The Rust SDK (the `velopack` crate) is called
from our own `main`, which is where Skathi's embedding puts us anyway.

**What the installer looks like (Windows).** There is no wizard. `Setup.exe`
shows a borderless, topmost, DPI-aware window with our image, then starts the
player. The image is a PNG or an animated GIF; alpha is honoured, so the window
can take the logo's shape. A 12 px progress bar in a colour we choose runs
along its bottom edge. Without an image Setup falls back to a plain system
progress dialog, so the image is not optional. Whatever a wizard would ask
belongs to V2-9.

**No folder choice.** Setup installs to `%LocalAppData%\<id>` and has no
directory page. `--installto <dir>` (with `--silent`) exists only on the
command line, for scripted deployment. That is deliberate: a folder the user
can write is what lets updates apply without elevation. Version 1's NSIS wizard
has a directory page, so this is a loss for whoever used it. **The answer is
the portable zip as a second, equal download**, which is what "my own folder"
usually means (`.portable` and `.data/` keep working). A bootstrapper of our
own with a folder picker was rejected:

- it is a second executable to sign and maintain;
- its progress could only be faked, because a silent Setup reports none;
- a folder outside the user's reach (`Program Files`) would break silent
  updates.

**macOS.** `vpk pack` puts `UpdateMac` inside `Contents/MacOS`, and Velopack
treats every `.app` as portable. A bundle dragged out of the disk image
therefore updates itself. The `dmgbuild` disk image stays the first download,
and Velopack's `.pkg` (skipped with `--noInst`) is not needed. A `.pkg` would
also need a Developer ID **Installer** certificate on top of the Application
one. For signing:

- `--signDisableDeep` signs only `UpdateMac` and the bundle. That fits today's
  chain, where `macos-sign.sh` signs the libraries first.
- Notarisation goes through a `notarytool` keychain profile
  (`--notaryProfile`), so CI has to store the existing API key in a profile
  first.

Still to run: an update of a bundle in `/Applications` from a standard
(non-admin) account.

**The update key.** Velopack checks each package against the SHA-1 and SHA-256
in the release feed, which it fetches over HTTPS. It verifies no signature.
Today's Tauri updater checks a minisign signature, so moving as-is is a step
down: whoever controls the bucket or the CDN would control every installed
player. Version 2 signs the feed with an ed25519 key and verifies it before
Velopack reads it. Like today's key, it exists only as a CI secret.

**Windows signing** is unchanged and still missing. Setup is unsigned, so
SmartScreen says "Unknown publisher" however good the splash looks. The
candidates:

- SignPath Foundation: free for open-source projects, signs from CI;
- Azure Artifact Signing: not available to individuals in every country.

`vpk pack` signs Setup, `Update.exe` and our binaries through a signing
command we give it.

**Moving version 1 installations.** The last Tauri release is a bridge:

- it downloads Velopack's Setup, runs it silently, and removes the NSIS
  installation through its own uninstaller;
- a copy installed into a folder the viewer chose is reinstalled into that same
  folder with `--installto`, not moved to `%LocalAppData%`;
- a portable copy is replaced by the version 2 portable zip in place, keeping
  `.portable` and `.data/`;
- settings move through V2-3's migration, run from Skathi's `prepare_data_dir`.

**Distribution.** A winget manifest from CI (`wingetcreate`). The Homebrew tap
stays as it is.

**Not done:**

- **A second update channel for the interface alone.** Skathi allows replacing
  `app.skpk` beside the executable, but Velopack's deltas already make an
  interface-only release small, and two channels would mean two version
  numbers.
- **MSIX and the Store**, for the reason in
  [distribution.md](distribution.md): a packaged application's file system is
  virtualised.

## Foundation

These carry no feature a viewer would name, but V2-5 to V2-9 are built on them.
They are cheapest done first, while there is little code to convert.

### V2-3. Settings in layers (planned)

Every setting resolves through three layers:

```
built-in default  <  preset (V2-7)  <  the viewer's own choice
```

Each setting also knows which layer its value came from. The hotkeys already
work this way: `keys.svelte.ts` stores only the viewer's overrides on top of
the defaults. Version 2 extends that to every setting. It gives four things:

- "reset this setting";
- "changed from the preset" marks in the settings sheet;
- switching a preset that keeps the viewer's own changes;
- a migration from version 1 that keeps only what the viewer actually changed.

**Storage must not be the web view's `localStorage`.** That limit is already
the "Durable storage beyond the webview" item in `ROADMAP.md`, and Skathi
removes the reason it existed. Settings go to a file owned by the application,
in a format with a version field, so V2-9 and future migrations have something
to read.

### V2-4. Design tokens from the first component (planned)

Version 1 has **614 colour literals in 53 files** and almost no custom
properties. A theme cannot be added to that without touching every surface. So
version 2 starts with tokens:

- **Colour**: surfaces, text levels, accent, danger, the seek bar's parts.
- **Material**: per surface: solid, frost or glass (V2-5).
- **Shape**: radii, with `corner-shape: squircle` where Skathi supports it.
- **Elevation and motion**: shadows, durations, easings.

Skathi supports what this needs: `var()`, `@property`, `@media`,
`prefers-color-scheme` and `light-dark()`, all updating live. A gate in the
spirit of `css-orphans` should reject a colour literal outside the token
files. Otherwise the 614 come back one at a time.

## Appearance

### V2-5. Themes: Classic and Glass (planned)

Two built-in themes on the same components. A theme is a set of token values
plus a material per surface. It is not a second layout.

- **Classic**: today's look, refined. The refinements are still to be listed:
  version 1's "frost" is density plus shadow, a stand-in for a blur it could
  not have. Classic can now use a real one.
- **Glass**: Liquid Glass-style panels (V2-6). It is the macOS default
  **only if** V2-6's measurements allow it; Classic is the default everywhere
  else.
- **An accent colour** chosen by the viewer, in both themes.

Light variants and themes written by viewers are under "Considered".

### V2-6. The glass material (to measure)

What Skathi offers, read in its source (0.4.x, November 2026):

- **Frost through plain CSS.** `backdrop-filter: blur()` works; `saturate()`
  and the other filter functions do not. There are gradients, `box-shadow` with
  `inset`, and continuous corners. That is enough for a frosted panel.
- **Refraction through a shader.** Skathi's `--shading` property applies an
  effect written in Slang, compiled to Metal, HLSL and WGSL. An effect marked
  `[SkBackdrop]` can sample the backdrop anywhere in the window
  (`backdropAt(uv)`), and it gets the signed distance to the element's rounded
  rectangle, whose gradient is the rim's normal.
- **A working starting point.** Skathi's Showcase template has a `lens.slang`
  described as "liquid glass": refraction at the rim, slight magnification in
  the middle, chromatic dispersion, a specular highlight. Skathi's devtools
  tune it live.
- **The backdrop should include the video.** `<sk-surface>` is drawn into the
  same frame, before the panels, and the backdrop is a snapshot of that frame.
  This was read in the code and **has not been run**.
- **It is an imitation, not the system material.** Skathi has no
  `NSGlassEffectView`. Its only native window material on macOS is a
  behind-window `NSVisualEffectView`. How the glass behaves is ours to tune,
  not Apple's.

**The cost is the open question.** A frame with any backdrop effect is rendered
into an intermediate texture and copied to the drawable. A `--shading` backdrop
effect snapshots **the whole window**, not the region under the element. With
video underneath, that happens on every video frame while the panels are
shown, which can mean 4K at 120 Hz on a laptop. The controls hide during
playback and the glass then costs nothing, so the expensive case is exactly
the one a viewer is interacting with.

**Before Glass can be a default, measure** on both platforms, with a panel using
`lens.slang` over a playing `<sk-surface>`:

1. that the glass refracts the video at all;
2. GPU time per frame, panels shown against hidden;
3. cadence, by the method in [native-frames.md](native-frames.md), "Frame
   pacing", at 1080p24 and 2160p at the display's full refresh rate;
4. power on battery on macOS;
5. HDR content behind the glass. Backdrop values above 1.0 (scRGB or EDR)
   would make the glass glow, so the shader has to tone-map what it samples.

**Decision rule:** if cadence with the panels shown matches cadence without
them, Glass is the default on macOS 26 and later. Otherwise the default is
frost, and Glass stays an option.

**Ask Skathi** for a region-limited snapshot for `--shading` backdrop effects.
`backdrop-filter` already snapshots only the blurred rectangles plus the blur's
reach.

**Always:**

- the system's "Reduce transparency" (macOS) and "Transparency effects"
  (Windows) switch Glass and frost to solid;
- text on glass gets a tint driven by the backdrop's luminance, because a white
  label over a bright sky is unreadable whatever the refraction looks like.

## Presets

### V2-7. Presets: "close to VLC", "close to mpv", … (planned)

A viewer coming from another player chooses a preset: once on first run
(V2-9), and at any time later in the settings. The preset sets the defaults
that player's users are used to. It is the second layer of V2-3, so the
viewer's own changes stay on top and survive switching presets.

**Prior art, and why we choose VS Code's model.**

- **IINA** ships its key sets as read-only `.conf` files: IINA, mpv, VLC and
  two Movist sets. Editing one means duplicating it first, and the copy no
  longer tracks the original.
- **VS Code** keeps one active keymap and stores the user's changes as a list
  of differences on top of it. "Changed from the preset" then means "has an
  override", and switching keymaps keeps the changes.

We follow VS Code.

**A preset is more than keys.** What makes a player feel familiar:

- the wheel: volume or seeking;
- a single click on the picture: pause (MPC-HC, YouTube, probably PotPlayer)
  or nothing (mpv, VLC);
- seek and volume steps;
- maximum volume: mpv 130 %, VLC 125 %, IINA 100 %;
- resume position, playing the folder's next file, what happens at the end of
  a file;
- screenshot format.

**Platforms:**

| Preset | Windows | macOS | Notes |
|---|---|---|---|
| Frame Player | ✓ | ✓ | The built-in defaults; also what a version 1 viewer is migrated to |
| mpv | ✓ | ✓ | mpv binds no Cmd keys, so the app menu's Cmd shortcuts stay as they are |
| VLC | ✓ | ✓ | Two variants: VLC's own macOS defaults use Cmd chords and different keys for tracks and subtitle delay |
| YouTube | ✓ | ✓ | J/K/L, number keys seek to 0–90 % |
| MPC-HC / MPC-BE | ✓ | – | Ctrl and Alt chords; Ctrl+arrows collide with Mission Control on macOS |
| PotPlayer | ✓ | – | No official key list, see below |
| IINA, QuickTime | – | ✓ | Cmd chords would collide with MPC-style Ctrl bindings on Windows |

**Rules:**

- **Name them "close to …".** None of those players has exactly our set of
  actions. IINA's VLC set says the same in its header.
- **Keep physical keys.** mpv binds characters (`<`, `>`, `{`, `}`, `#`);
  we bind `e.code`. A preset translates them to Shift plus the US-layout
  physical key, the same convention as our key badges.
- **Our own actions.** An action the source player lacks keeps our default key
  if the preset leaves it free, and is otherwise unbound. It is never silently
  dropped from the editor.
- **Sources of truth:** mpv's `etc/input.conf`; VLC's `libvlc-module.c`
  (3.0.x); MPC-HC's `AppSettings.cpp`; IINA's `iina/config/*.conf`; YouTube's
  and Apple's help pages. **PotPlayer has no official list**, and the
  secondary sources disagree, for example on the speed-reset key. Check its
  preset against the program itself before shipping it.

### V2-8. Importing another player's configuration (considered)

The VS Code analogue is "Migrate Keyboard Shortcuts from…". Read the viewer's
actual mpv `input.conf` / `mpv.conf`, or VLC's `vlcrc` hotkeys, and turn it
into overrides on top of the matching preset. Worth it only after V2-7 exists,
and only for the formats whose meaning is unambiguous.

## First run

### V2-9. The welcome screen (planned)

Shown once after installation, and again in a shorter form after a version
that adds a step.

**What triggers it.** An "onboarding done, version N" field in the settings
file (V2-3). Velopack's first-run hook only makes the screen appear immediately
after setup. It cannot be the trigger:

- a portable copy never runs an installer;
- a macOS copy dragged out of a disk image has no installer either;
- a version 1 viewer upgrading has run an installer before.

**Steps** (each skippable, each repeated in the settings sheet):

1. language;
2. theme, previewed live on a video frame (V2-5);
3. preset (V2-7);
4. file types: the player in Default apps on Windows, the handlers on macOS;
5. what leaves this machine (the catalog, the install count), stated before
   any of it happens.

**A version 1 viewer** keeps their settings (V2-3 migration) and the
"Frame Player" preset. They see what is new and the theme choice, not the whole
setup again.

## Open questions

- What exactly does "refined" Classic change?
- Is a light variant worth it for a video player, where the content sets the
  room's brightness?
- Should a preset ever change layout (where the controls sit), or only
  behaviour? The answer so far is behaviour only.
- Does a Velopack update of a bundle in `/Applications` work from a standard
  (non-admin) account, where that folder is not writable? (V2-2)

## Considered

- **Themes written by viewers**: a token file in a versioned format, import and
  export. It needs V2-4's token names to be stable first, because they become
  a public format.
- **A light variant** of each theme, through `light-dark()`.
- **Choosing the default theme by OS version**: Glass only where the system
  itself is Liquid Glass (macOS 26+). This is the current plan in V2-5; listed
  here in case the system look moves on again.

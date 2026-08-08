# Shipping

What it takes to hand this to somebody who did not build it: signatures, the two
operating systems' opinions about unknown software, the update path, and what a
store listing would cost.

## macOS

### The bundle must be sealed, even by nothing

Gatekeeper has **two distinct rejection paths** and they produce different
interfaces:

| verdict | what the user sees |
|---|---|
| `code has no resources but signature indicates they must be present` | *"is damaged and can't be opened. You should move it to the Trash"* — with **no way out in the interface** |
| `rejected` | *"Apple cannot check it for malicious software"* — with an **Open Anyway** button in Privacy & Security |

The escape hatch exists for an *untrusted identity*, not for an *invalid
signature*. A build with no bundle signature at all — only the one the linker
puts on an arm64 binary so that it can execute — takes the first path, and the
only workaround is removing the quarantine attribute from a terminal. That is
not a thing to ask of anyone.

An **ad-hoc signature** fixes it. It certifies nothing, but it seals the bundle,
which moves the refusal to the second path. Paired with hardened runtime turned
**off**: the bundler defaults it on, it is only needed for notarisation, and
without entitlements it breaks the Lua runtime mpv uses for the zoom/pan script.

Two things to know about the result: the signature covers the inner binary, then
the bundle, and only then are the disk image and the update archive built — so
both artefacts inherit the seal. And since macOS 15, right-click → Open no
longer bypasses Gatekeeper; Privacy & Security is the only route, so the release
notes have to say so.

### A real Developer ID

A paid developer membership plus notarisation removes the warning entirely. It
also brings hardened runtime — required for notarisation — which means
entitlements for the JIT the Lua runtime needs, and a notarisation step in
continuous integration (submit, wait, staple). That is a subscription and a
pipeline change rather than code.

### Private API

The window transparency the whole embedding model depends on uses a private
system interface, which is fine for direct distribution and disqualifies the
application from the Mac App Store. That trade was made knowingly: the App Store
is not a channel this player needs, and the alternative is not having the
architecture.

### The disk image

The bundler's own image builder drives the Finder over AppleScript to place the
icons, which a headless machine has no session for — it passes silently and
ships an image that opens as a plain folder. The image is therefore built in a
separate step by a tool that writes the layout directly.

## Windows

### SmartScreen reputation

Reputation attaches to **two** things: the file hash and the publisher identity
from the signing certificate.

- **Unsigned** — there is no identity, so every build starts from zero and for
  a niche application the warning never goes away.
- **Signed with an organisation-validated certificate** — reputation accrues to
  the publisher and is inherited by new files. The warm-up is once per
  *certificate*, not once per release; the duration is unpublished and reported
  anywhere from hundreds to thousands of installs.
- **Signed with an extended-validation certificate** — the only thing that buys
  immediate reputation, which is what the price difference is for.

Signing without reputation is still worth something: the dialog names the
publisher instead of saying "Unknown publisher".

Since the 2023 CA/Browser Forum rules, private keys must live in hardware — a
USB token, which is useless for automated builds, or a cloud signing service,
which is not.

### The update takes seconds, and where they go

| | macOS | Windows |
|---|---|---|
| artefact | `.app.tar.gz`, gzip | `-setup.exe`, solid LZMA |
| what the updater does | unpack, swap the application directory | launch a separate installer process |
| removes the old version first | no | **yes** — runs the previous uninstaller and waits |

So a Windows update deletes the previous installation and writes the new one
back, decompressing an LZMA payload, where macOS unpacks a gzip archive over the
old directory. Levers, cheapest first:

- **Quiet install mode** removes the installer window. Not faster, but it turns
  "the app closes, a foreign window appears, the app comes back" into "the app
  closes and comes back".
- **Announce the update before starting it.** The project's own rule that a slow
  operation must say so first has to be applied *before* the install call,
  because on Windows no code after that call ever runs — the installer kills the
  process from inside it. Anything that must survive an update (the resume
  snapshot) has to be written first for the same reason.
- **Compression.** A faster codec would decompress in a fraction of the time and
  grow the download by roughly half. That trades processor seconds for network
  seconds and is only a win on a fast link.
- **The size itself.** On Windows libmpv and the thumbnail sidecar link
  *separate* FFmpeg copies, where macOS points both at one set, so the installer
  ships FFmpeg twice. Deduplicating cuts download and decompression at once.

### A store listing

Packaging for the Microsoft Store is technically possible and buys installation
trust and discovery. What stands in the way is the same thing that makes the
player work: the installation is per-user and unpackaged, several components are
laid out beside the executable at build time, and a packaged application's file
system is virtualised. It is a project, not a checkbox.

## Releases

The version lives in **five files** — the package manifest and its lockfile, the
bundler configuration, the crate manifest and its lockfile — and they cannot be
collapsed into one. The bundler's version field accepts a path to a package
manifest instead of a literal, but the release pipeline reads that literal to
decide whether a push *is* a release, so making it indirect would leave the gate
with nothing to compare; and the crate manifest cannot read a version out of
JSON.

A script writes all five, with one anchored line per file rather than a
structured round-trip (which reformats hand-written inline arrays), and **every
anchor must match exactly once** — a bump that silently skipped a file is the
whole failure being prevented. Run with no argument it reports instead of
writing and exits non-zero on disagreement, which is worth doing before a
release: this drift is invisible, and the first run of the script found a
lockfile that had been sitting at the initial version since the first commit.

The pipeline is gated on the version changing: build → sign → upload the
installer and the update manifest → publish the release. Signing keys and the
storage credentials are repository secrets; an absent secret is not a build
failure, the feature that needs it simply reports that this build has no key.

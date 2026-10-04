# Shipping

What it takes to hand this to somebody who did not build it: signatures, the two
operating systems' opinions about unknown software, the update path, and what a
store listing would cost.

## macOS

The build is signed with a Developer ID and notarised. What follows is why the
interim arrangement looked the way it did, and — more usefully — the three
things that made turning notarisation on cost a day rather than an afternoon.

### The chain, and why its order is not free

Sign the native libraries → build → notarise the app → staple → repack the
updater archive → build the disk image → sign, notarise and staple that.

Two of those placements are load-bearing. The **libraries are signed before the
build**, because they reach the bundle as declared resources: copied in,
carrying whatever signature they already had, which is the ad-hoc one applied
when their load commands were rewritten. Notarisation inspects every Mach-O file
in the bundle and refuses an ad-hoc one on identity, so they have to be right
before they are copied; signing them inside the finished bundle would break its
seal and force the image and the updater archive to be rebuilt around a
signature that had just been replaced.

The **updater archive is repacked after stapling**, because the bundler writes
it during the build — before the ticket exists. Left alone, the disk image that
first-time users download is notarised while every automatic update carries an
unstapled bundle. That failure reaches only existing users, weeks later, which
is the worst shape a release bug can have.

### The hardened runtime is the hard part, not the certificate

Notarisation requires the hardened runtime, and the hardened runtime is two
separate restrictions. Both were measured against a probe that loads the bundled
media library with the zoom/pan script under each candidate signature.

**Executable memory.** The scripting runtime is a JIT, and the hardened runtime
kills a process that executes a page it did not sign. The entitlement whose name
matches the problem — *allow JIT* — **does not fix it**: it authorises a
specific system call for mapping JIT memory, and this build of the runtime does
not use it. The process dies with a bad-access exception, "Invalid Page", inside
a private executable region. The blunter entitlement, which permits unsigned
executable memory outright, is what works, and it is the only one shipped.

**Library validation.** The hardened runtime also requires every library the
process loads to carry the same team identifier as the process. With a real
certificate that is satisfied for nothing, since the same identity signs the
libraries. With an ad-hoc signature it cannot be satisfied at all — ad-hoc code
has no team identifier — so the app dies at launch on the first library. The
conclusion is that **the hardened runtime is only turned on when there is a
certificate to pair it with**; a build from a clean checkout with no Apple
account still produces a working, ad-hoc-sealed app.

Three traps sit around that, each of which costs an hour on its own:

- Turning the hardened runtime *off* in configuration does not turn it off. The
  bundler signs with it whenever an entitlements file is named, whatever the
  flag says, so the override has to clear both. (Verified against the signing
  tool directly: entitlements without the runtime option produce a plain ad-hoc
  signature, so this is the bundler's doing, not the tool's.)
- A probe that loads the libraries dynamically survives every one of these
  failures. Testing the library set in isolation proves nothing about the
  bundle; only launching the bundle does.
- Deep signature verification passes on the broken bundle, in silence. The
  symptom is a crash dialog offering to send a report to Apple.

### Gatekeeper's two paths, which still decide the unsigned case

They produce different interfaces:

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
which moves the refusal to the second path — and since macOS 15, right-click →
Open no longer bypasses Gatekeeper, so Privacy & Security is the only route
left and anything describing that build has to say so.

This is no longer what the releases carry, but it is still what anyone building
from the repository gets, and it is the reason the ad-hoc path is maintained
rather than merely tolerated: the alternative for them is not a warning, it is
an application the interface offers no way to open.

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

### Homebrew

The package manager's own repository of graphical applications will not take
this one yet: its casks have to clear a popularity bar, measured in stars and
forks, that a project with a handful of either does not reach. A tap of one's
own has no such requirement and is a public repository holding a single file.

It does have to be a *separate* repository, or very nearly. The one-argument
form of the tap command expands to a fixed repository name, so a cask living in
the application's own repository can only be reached by the two-argument form
with a full URL — which is the instruction people copy wrong, and which makes
every user's routine update fetch the whole application repository for the sake
of one file. The separate repository also keeps the release automation's write
access pointed at a repository containing nothing but that file, rather than at
the branch holding the code.

**The two update mechanisms do not fight, and the cask says so.** Marking the
application as self-updating is what tells the package manager to leave it
alone: an upgrade run skips it entirely unless explicitly told to be greedy. So
the package manager is the way in and the way out, the player's own signed
updater is the update channel, and the only visible consequence is that the
recorded version goes stale — which is how every self-updating application in
that repository behaves.

Two things the cask has to get right that are properties of this project rather
than of packaging. It points at the **release asset, not the object store**: the
store keeps five versions and the release keeps its files forever, and a
download that no longer exists is worse than a version that is merely old. And
it declares the build **Apple Silicon only**, since that is the only macOS
target built — without the declaration an Intel machine installs an application
it cannot launch, with it the refusal names a reason.

Keeping the cask current is the last step of the release run, placed there under
the same rule as the storage sweep: a step must not stand in front of something
more important than itself. A tap that failed to bump hands out the previous
version; a release that was never created is an artifact nobody can obtain. Its
credential is a deploy key rather than a personal token — it cannot expire, and
it reaches exactly one repository by construction instead of by the scope
somebody remembered to set.

## Windows

### SmartScreen reputation

Reputation attaches to **two** things: the file hash and the publisher identity
from the signing certificate.

- **Unsigned** — there is no identity, so every build starts from zero and for
  a niche application the warning never goes away.
- **Signed with an organization-validated certificate** — reputation accrues to
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

### The update takes seconds, and where they went

This is where the Windows update started, and the table is kept because it is
what the in-place path below was written against:

| | macOS | Windows, originally |
|---|---|---|
| artifact | `.app.tar.gz`, gzip | `-setup.exe`, solid LZMA |
| what the updater does | unpack, swap the application directory | launch a separate installer process |
| removes the old version first | no | **yes** — runs the previous uninstaller and waits |

So a Windows update deleted the previous installation and wrote the new one
back, decompressing an LZMA payload, where macOS unpacks a gzip archive over the
old directory. The levers, cheapest first, were:

- **Quiet install mode** removes the installer window. Not faster, but it turns
  "the app closes, a foreign window appears, the app comes back" into "the app
  closes and comes back". Shipped, and still in `tauri.conf.json` as
  `installMode: passive` — which is what the fallback below uses.
- **Announce the update before starting it.** The project's own rule that a slow
  operation must say so first had to be applied *before* the install call,
  because no code after that call ever ran — the installer kills the process
  from inside it. Anything that must survive an update (the resume snapshot) had
  to be written first for the same reason.
- **Compression.** A faster codec would decompress in a fraction of the time and
  grow the download. Measured below.
- **The size itself.** On Windows libmpv and the thumbnail sidecar link
  *separate* FFmpeg copies, where macOS points both at one set, so the installer
  ships FFmpeg twice. Deduplicating cuts download and decompression at once, and
  is the one lever here still untouched.

### Replacing the files instead of reinstalling them

What ships now is a second Windows artifact — the installation as a zip — which
the player unpacks into its own directory and swaps file by file. No installer
window, no uninstall pass, and code after the call runs, which is what lets the
player relaunch itself the way macOS always has. The rules are in
[`rules/build-and-release.md`](rules/build-and-release.md); what follows is why
it looks like this.

**What Windows actually allows was measured before anything was written**, in
isolation, with a DLL loaded through `LoadLibrary` and a copy of a real
executable running:

| with the process running and the image mapped | |
|---|---|
| rename a loaded DLL inside its directory | works |
| rename the running `.exe` | works |
| create a new file at the name just vacated | works |
| **delete** the renamed image | **access denied** |
| delete it once the process has exited | works |
| rename a *directory* holding a loaded DLL | works |

Two of those decided the design. Because a renamed image cannot be deleted until
the process holding it exits, the leftovers are somebody else's job: the
relaunched copy is given `--after-update=<pid>` and waits for that process before
sweeping. And the last row is the one usually told the other way round — a
directory with a loaded DLL in it *can* be renamed — so per-file renaming is a
choice rather than a necessity, made because it is what can be rolled back one
file at a time.

**Compression was a measurement, not a preference.** The same 259.8 MB
installation, in one archive each:

| | size | needs |
|---|---|---|
| `-setup.exe`, solid LZMA (what the installer is) | 85.6 MB | — |
| zip, deflate level 9 | **105.9 MB** | nothing — `flate2` is already in the tree |
| zip, zstd level 19 | 93.0 MB | a C codec in the reader, and a zip writer of our own |

Deflate costs about twenty megabytes against the installer and buys back the
uninstall pass and the LZMA decompression, so the update is faster end to end on
anything but a slow link — and the reader is the `zip` crate that
`tauri-plugin-updater` already pulls in, with one pure-Rust feature added.
zstd would have closed most of that gap; what it wanted in exchange was a C
library in the client and, on the packaging side, a zip writer written here,
because nothing that ships with Windows writes a zstd zip. That trade is
recorded rather than taken, and the number is here for whoever wants to revisit
it. The other half of the gap is the FFmpeg duplication above, which would
shrink both artifacts at once and is the better lever of the two.

**The uninstaller is why the file set is frozen.** The NSIS uninstaller deletes a
list of paths generated when it was built, then removes the directories it knows
about and nothing else; only an installer run writes a new one. So an in-place
update that added or dropped a path would leave an uninstaller unable to clean up
after itself — and a release that renames a library, which a major FFmpeg bump
does, would leave ninety megabytes behind after an uninstall with nothing to say
so. Three ways out were considered:

- **Ship `uninstall.exe` in the payload.** It exists only after an installer has
  run, so building the payload would mean running the installer into a staging
  directory in CI — which also writes uninstall-registry entries and shortcuts,
  and could not be reproduced on a developer's machine without clobbering their
  own installation's entry.
- **An `RMDir /r "$INSTDIR"` hook in the uninstaller.** Tauri supports the hook,
  and it would make the uninstaller correct for ever. It would also delete
  whatever else is in that directory — and `$INSTDIR` is whatever the person
  typed during installation, so somebody who installed into `D:\Utilities`
  rather than `D:\Utilities\Frame Player` would lose `D:\Utilities`.
- **Refuse the swap when the set changes**, which is what runs. The release that
  changes the file set goes through the installer once, the installer rewrites
  the uninstaller, and every release after it is in place again.

The set on disk is read by *walking the installation* rather than from a manifest
a previous version left behind, which is what makes the first in-place update
work with no transition release — and it is also why the payload's manifest sits
at the root of the zip, outside the folder that gets installed: a manifest
installed beside the binaries would itself be a path the uninstaller does not
know, and the very first swap would refuse itself.

That comparison is also the safety property worth stating plainly: **if the
payload's layout ever stops matching what the installer lays down, every client
refuses the swap and falls back to the installer.** Getting the packaging wrong
costs a release its new update path; it cannot produce a broken installation.

### Testing the Windows update

The whole client path runs locally, against a real installation layout, with no
release and no access to the project's signing key — which matters because the
alternative is finding out from the first person to press the button.

A **debug build only** reads `FP_UPDATE_ENDPOINT` and `FP_UPDATE_PUBKEY` from the
environment, so a throwaway key and a loopback server stand in for R2 while the
signature check stays in the loop exactly as it is in a release. A release binary
ignores both, which is the point of the gate: nothing in the environment can
point a shipped player at another manifest or another signing key. The plugin's
own refusal of `http://` endpoints is lifted under the same condition, which is
why the server can be plain HTTP.

```bash
bun run tauri build --debug --no-bundle
powershell -ExecutionPolicy Bypass -File scripts/update-test.ps1 -WithRegistry
```

The script generates the key, builds and signs a payload with
`pack-windows-update.ps1` — the same command the release workflow runs — serves
it on loopback, and launches a *copy* of a real installation with a few bytes
appended to its executable and one of its scripts, so that the swap has something
to do. (Trailing bytes on a PE file are overlay data and the loader ignores
them.) Nothing outside the temporary directory changes, except with
`-WithRegistry`, which points the uninstall entry at the staged copy after
exporting it to a `.reg` file and puts it back at the end.

`-Auto <seconds>` then runs the update through the same two commands the button
calls, so the swap can be exercised with the libraries mapped and a video playing
without a hand on the mouse (`FP_UPDATE_AUTO`, the same shape as the
`FP_DLNA_PLAY` self-test). What that leaves untested is the frontend's two lines
— the progress listener and the resume snapshot — and the release workflow's own
half, for which the payload being built by the script CI runs is the whole of the
mitigation.

Two traps in the scripts themselves, both of which cost an hour:

- **A `.ps1` in this repository has no byte-order mark, so Windows PowerShell 5.1
  decodes it as the ANSI code page** — and the third byte of a UTF-8 em dash
  lands on U+201D, which PowerShell accepts as a closing double quote. A string
  containing one ends there, the next quote in the file opens another, and
  everything between them — a whole function, in the case that found this —
  parses as a string literal and ceases to exist. It is not a syntax error: the
  script runs and reports the function as an unknown command. Comments are safe,
  since those end at the line break regardless. Hence: nothing but ASCII inside a
  quoted string.
- **`$env:X = ''` deletes the variable** rather than setting it to empty, so
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` set that way leaves the signer prompting
  for a password with nobody to answer, and the script hangs with no output. The
  password goes on the command line (`--password=`) instead. In the workflow,
  where `""` in YAML really is an empty variable, the environment is fine.

### A store listing

Packaging for the Microsoft Store is technically possible and buys installation
trust and discovery. What stands in the way is the same thing that makes the
player work: the installation is per-user and unpackaged, several components are
laid out beside the executable at build time, and a packaged application's file
system is virtualised. It is a project, not a checkbox.

## Releases

The version lives in **four files** — the package manifest, the
bundler configuration, the crate manifest and its lockfile — and they cannot be
collapsed into one. The bundler's version field accepts a path to a package
manifest instead of a literal, but the release pipeline reads that literal to
decide whether a push *is* a release, so making it indirect would leave the gate
with nothing to compare; and the crate manifest cannot read a version out of
JSON.

A script writes all four, with one anchored line per file rather than a
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

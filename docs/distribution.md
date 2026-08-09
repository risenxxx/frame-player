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

### The update takes seconds, and where they go

| | macOS | Windows |
|---|---|---|
| artifact | `.app.tar.gz`, gzip | `-setup.exe`, solid LZMA |
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

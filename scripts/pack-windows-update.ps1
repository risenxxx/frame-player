<#
.SYNOPSIS
  Builds the Windows in-place update payload: FramePlayer_<version>_x64.zip
  and the manifest the player checks it against.

.DESCRIPTION
  The zip is the second Windows release artifact. The first one — the NSIS
  installer — is still what a new user downloads and still what every client
  older than the in-place update reads out of `latest.json`; this one is what
  the player swaps into its own installation directory without running an
  installer at all (see src-tauri/src/update.rs and docs/distribution.md).

  Its layout is the installation's layout, because that is what the swap puts
  on disk file for file:

      FramePlayer_1.25.0_x64.zip
      ├── update-manifest.json      <- NOT installed; the swap reads and drops it
      └── Frame Player/             <- one root folder, the installation itself
          ├── frameplayer.exe
          ├── avcodec-62.dll
          ├── lib/…
          └── …

  The root folder exists for the portable download, which is the same archive:
  a flat zip would explode into whatever directory it was opened in. The
  manifest sits outside it for a sharper reason — the file set of an
  installation has to keep matching the delete list the NSIS uninstaller baked
  in at install time, so an in-place update may not add a path. A manifest
  installed beside the binaries would be exactly such an added path, and the
  very first swap would refuse itself.

  The file list is derived from `bundle.resources` in tauri.conf.json and the
  main binary — the same inputs the bundler lays out — rather than from a
  second list kept here. `-CompareWith <dir>` checks the result against a real
  installation; that comparison is the drift check and is worth running
  whenever the resource map changes. It is not a gate: drift cannot ship a
  broken update, because the player compares the incoming file set with what is
  on disk and falls back to the installer if they differ.

.PARAMETER Version
  Defaults to the version in tauri.conf.json.

.PARAMETER Exe
  The built main binary. Defaults to src-tauri/target/release/<crate name>.exe.

.PARAMETER OutDir
  Where the zip and the staging tree go. Defaults to ./dist.

.PARAMETER CompareWith
  An existing installation directory to diff the staged file list against.
  Fails if a path is missing from either side (`uninstall.exe`, which belongs
  to the installer and is never swapped, is ignored).

.PARAMETER Portable
  Build the portable download instead of the update payload: the same files,
  plus the `.portable` marker that tells the player to keep its state beside
  itself, and without the manifest - nothing updates from this archive, so
  there is nothing for a manifest to describe, and a human unpacking it should
  find one folder and no bookkeeping.

  It is deliberately not signed and deliberately not uploaded to R2: it is a
  download, not an update payload, and a GitHub release keeps its assets where
  the update bucket is pruned to the last few versions. A portable copy updates
  itself from the ordinary payload, because its file set is the same one.

.EXAMPLE
  bun run tauri build
  powershell -ExecutionPolicy Bypass -File scripts/pack-windows-update.ps1 `
    -CompareWith "$env:LOCALAPPDATA\Frame Player"
#>
param(
  [string]$Version,
  [string]$Exe,
  [string]$OutDir = 'dist',
  [string]$CompareWith,
  [switch]$Portable
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

# Nothing but ASCII inside a quoted string in this file, however tempting a dash
# is. The repository stores .ps1 without a byte-order mark, Windows PowerShell
# 5.1 therefore decodes it as the ANSI code page, and the third byte of a UTF-8
# em dash lands on U+201D - which PowerShell accepts as a closing double quote.
# The string ends there, the next quote in the file opens a new one, and
# everything between the two (a whole function, in the case that found this)
# parses as a string literal and simply ceases to exist. It is not a syntax
# error: the script runs and reports the function as an unknown command.
# Comments are safe, since those end at the line break regardless.

# Windows PowerShell 5.1 does not have the compression types loaded: ZipFile and
# the CreateEntryFromFile extension are in .FileSystem, ZipArchiveMode in the
# other assembly. PowerShell 7 has both already, and whether Add-Type will
# resolve a framework name there is not worth depending on - so the loads are
# attempts and the types themselves are what gets checked.
foreach ($assembly in @('System.IO.Compression', 'System.IO.Compression.FileSystem')) {
  try { Add-Type -AssemblyName $assembly -ErrorAction Stop } catch { }
}
foreach ($type in @('System.IO.Compression.ZipFile', 'System.IO.Compression.ZipFileExtensions',
                    'System.IO.Compression.ZipArchiveMode')) {
  if (-not ($type -as [type])) { Write-Error "$type is not available in this PowerShell" }
}

$repo = Split-Path -Parent $PSScriptRoot
$tauriDir = Join-Path $repo 'src-tauri'
$conf = Get-Content (Join-Path $tauriDir 'tauri.conf.json') -Raw | ConvertFrom-Json

if (-not $Version) { $Version = $conf.version }
$product = $conf.productName

# The main binary is named after the crate, not after the product: Tauri uses
# `mainBinaryName` when it is set and the Cargo package name otherwise, and
# this configuration sets neither — hence `frameplayer.exe` rather than
# `Frame Player.exe`, which is also what the relaunch after a swap has to spawn.
if (-not $Exe) {
  $cargo = Get-Content (Join-Path $tauriDir 'Cargo.toml') -Raw
  if ($cargo -notmatch '(?m)^name\s*=\s*"([^"]+)"') {
    Write-Error 'could not read the crate name out of src-tauri/Cargo.toml'
  }
  $Exe = Join-Path $tauriDir "target/release/$($Matches[1]).exe"
}
if (-not (Test-Path -LiteralPath $Exe)) {
  Write-Error "no built binary at $Exe - run 'bun run tauri build' first"
}

<#
  One entry of `bundle.resources` resolved the way the bundler resolves it: the
  pattern's leading glob-free segments are the base, and what a matched file's
  path relative to that base is gets appended to the destination. So
  `lib/**/*` -> `lib/` keeps subdirectories, and `ffmpeg/bin/*.dll` -> `./`
  flattens into the root. Returns @{ From; To } pairs with `/` separators.
#>
function Resolve-Resource {
  param([string]$Pattern, [string]$Dest)

  $segments = $Pattern -split '[\\/]'
  $baseParts = @()
  $restParts = @()
  foreach ($s in $segments) {
    if ($restParts.Count -eq 0 -and $s -notmatch '[\*\?\[]') { $baseParts += $s } else { $restParts += $s }
  }
  # A pattern with no glob at all names one file: its own directory is the base.
  if ($restParts.Count -eq 0) {
    $restParts = @($baseParts[-1])
    $baseParts = $baseParts[0..($baseParts.Count - 2)]
  }

  $base = Join-Path $tauriDir ($baseParts -join [IO.Path]::DirectorySeparatorChar)
  if (-not (Test-Path -LiteralPath $base)) {
    Write-Error "resource pattern '$Pattern' has no directory at $base"
  }
  $base = (Resolve-Path -LiteralPath $base).ProviderPath

  # The remaining segments as one regex over the `/`-joined relative path.
  # `**/` spans whole directories, `*` and `?` stop at a separator.
  $rx = ''
  foreach ($s in $restParts) {
    if ($s -eq '**') { $rx += '(?:[^/]+/)*'; continue }
    $part = [regex]::Escape($s) -replace '\\\*', '[^/]*' -replace '\\\?', '[^/]'
    $rx += $part + '/'
  }
  $rx = '^' + $rx.TrimEnd('/') + '$'
  # `**/x` has to match a bare `x` too, which the join above spells `(?:[^/]+/)*x`
  # already — but `**` as the last segment must not demand a trailing slash.
  $rx = $rx -replace '\(\?:\[\^/\]\+/\)\*\$', '.*$'

  $out = @()
  # -Force so that dotfiles are included: `lib/.libs-key` is one, and the
  # bundler ships it (glob's leading-dot literal requirement is off there too).
  foreach ($f in Get-ChildItem -LiteralPath $base -Recurse -File -Force) {
    $rel = $f.FullName.Substring($base.Length).TrimStart('\', '/') -replace '\\', '/'
    if ($rel -notmatch $rx) { continue }
    $to = ($Dest -replace '\\', '/').TrimEnd('/')
    if ($to -eq '.' -or $to -eq '') { $to = $rel } else { $to = "$to/$rel" }
    $out += @{ From = $f.FullName; To = $to }
  }
  if ($out.Count -eq 0) { Write-Error "resource pattern '$Pattern' matched nothing under $base" }
  return $out
}

# --- the file list ---------------------------------------------------------

$files = @(@{ From = (Resolve-Path -LiteralPath $Exe).ProviderPath; To = (Split-Path -Leaf $Exe) })
foreach ($p in $conf.bundle.resources.PSObject.Properties) {
  $files += Resolve-Resource -Pattern $p.Name -Dest $p.Value
}

$seen = @{}
foreach ($f in $files) {
  $key = $f.To.ToLowerInvariant()
  if ($seen.ContainsKey($key)) { Write-Error "two resources land on $($f.To)" }
  $seen[$key] = $true
}

# --- hash and zip ----------------------------------------------------------

if ([IO.Path]::IsPathRooted($OutDir)) { $outPath = $OutDir } else { $outPath = Join-Path $repo $OutDir }
New-Item -ItemType Directory -Force -Path $outPath | Out-Null

$entries = @()
$raw = 0
foreach ($f in ($files | Sort-Object { $_.To })) {
  $info = Get-Item -LiteralPath $f.From -Force
  $raw += $info.Length
  $entries += [ordered]@{
    path   = $f.To
    size   = $info.Length
    sha256 = (Get-FileHash -LiteralPath $f.From -Algorithm SHA256).Hash.ToLowerInvariant()
  }
}

$doc = [ordered]@{
  version = $Version
  product = $product
  root    = $product
  files   = @($entries)
}
# -Depth matters: the default of 2 would render each file entry as a type name.
$json = $doc | ConvertTo-Json -Depth 5
# No byte-order mark: the player parses this with serde_json, which refuses one
# outright, and 5.1's `utf8` encoding writes one. Written to a temp file rather
# than into the output directory, because everything the release workflow leaves
# in dist/ is uploaded as it stands.
$manifestFile = [IO.Path]::GetTempFileName()
[IO.File]::WriteAllText($manifestFile, $json, (New-Object Text.UTF8Encoding $false))

$suffix = ''
if ($Portable) { $suffix = '-portable' }
$zip = Join-Path $outPath "FramePlayer_${Version}_x64${suffix}.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -Force -LiteralPath $zip }
# Entry by entry rather than CreateFromDirectory, for two reasons. It saves
# copying 260 MB into a staging tree only to read it straight back. And
# CreateFromDirectory on .NET Framework writes entry names with **backslashes**
# - which the zip format does not allow (APPNOTE 4.4.17.1 says the separator is
# a forward slash) and which no conforming reader finds by name. The player's
# reader refused such a payload outright, so the only cost was an afternoon;
# left unnoticed it would have been a release whose in-place update silently
# fell back for everyone.
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
  $level = [IO.Compression.CompressionLevel]::Optimal
  if ($Portable) {
    # Present is all it has to be; the text is for whoever opens it wondering.
    $entry = $archive.CreateEntry("$product/.portable", $level)
    $writer = New-Object IO.StreamWriter($entry.Open())
    try {
      $writer.Write("Frame Player keeps its settings, watch history and caches in the .data`r`nfolder beside this file. Delete this file to make this copy behave like an`r`nordinary installation and use the Windows user profile instead.`r`n")
    } finally { $writer.Dispose() }
  } else {
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
      $archive, $manifestFile, 'update-manifest.json', $level)
  }
  foreach ($f in ($files | Sort-Object { $_.To })) {
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
      $archive, $f.From, "$product/$($f.To)", $level)
  }
} finally {
  $archive.Dispose()
  Remove-Item -Force -LiteralPath $manifestFile
}

$zipSize = (Get-Item -LiteralPath $zip).Length
Write-Output ("{0}: {1} files, {2:N1} MB -> {3:N1} MB ({4:N0}%)" -f `
  (Split-Path -Leaf $zip), $entries.Count, ($raw / 1MB), ($zipSize / 1MB), ($zipSize / $raw * 100))
foreach ($e in $entries) { Write-Output ("  {0,12:N0}  {1}" -f $e.size, $e.path) }

# --- the drift check -------------------------------------------------------

if ($CompareWith) {
  if (-not (Test-Path -LiteralPath $CompareWith)) { Write-Error "nothing to compare with at $CompareWith" }
  $installed = @(Get-ChildItem -LiteralPath $CompareWith -Recurse -File -Force |
    ForEach-Object { $_.FullName.Substring((Resolve-Path -LiteralPath $CompareWith).ProviderPath.Length).TrimStart('\', '/') -replace '\\', '/' } |
    Where-Object { $_ -ne 'uninstall.exe' })
  $staged = @($entries | ForEach-Object { $_.path })
  $onlyStaged = @(Compare-Object $staged $installed | Where-Object { $_.SideIndicator -eq '<=' } | ForEach-Object { $_.InputObject })
  $onlyInstalled = @(Compare-Object $staged $installed | Where-Object { $_.SideIndicator -eq '=>' } | ForEach-Object { $_.InputObject })
  if ($onlyStaged.Count -or $onlyInstalled.Count) {
    foreach ($p in $onlyStaged) { Write-Output "  only in the zip:       $p" }
    foreach ($p in $onlyInstalled) { Write-Output "  only in $CompareWith : $p" }
    Write-Error 'the staged layout and the installation disagree: the in-place update would refuse itself and fall back to the installer'
  }
  Write-Output "layout matches $CompareWith ($($staged.Count) files)"
}

<#
.SYNOPSIS
  Runs a real Windows in-place update against a staged installation, locally,
  with no release and no access to the signing key.

.DESCRIPTION
  What this covers is everything the client does: the manifest check, the signed
  download and its verification, the unpack, the file-set comparison, the swap
  with the libraries loaded and a video playing, the uninstall entry, the
  relaunch past the single-instance guard, and the sweep of the renamed files on
  the launch after. What it does not cover is the release workflow's half -
  building and signing the payload in CI - for which `pack-windows-update.ps1`
  is run here by the same command the workflow runs.

  It works without the project's signing key because the player accepts
  `FP_UPDATE_ENDPOINT` and `FP_UPDATE_PUBKEY` from the environment **in a debug
  build only** (see `updater()` in src-tauri/src/update.rs). So this generates a
  throwaway key, signs the payload with it, serves it over loopback, and points
  one debug build at both. A release build ignores all of it.

  Nothing touches the real installation. The staged copy lives under the
  temporary directory, is built by copying an installation rather than by
  installing anything, and the only thing outside it that changes is the
  uninstall entry - and then only with -WithRegistry, which saves it to a .reg
  file first and puts it back at the end.

  The staged copy's own executable and one of its scripts get a few bytes
  appended, so that they differ from the payload's and the swap has something to
  do. Trailing bytes on a PE file are overlay data and the loader ignores them.

.PARAMETER From
  An installation to copy. Defaults to whatever the uninstall entry points at,
  and then to %LOCALAPPDATA%\Frame Player.

.PARAMETER Version
  The version to announce. Defaults to the configured version with its patch
  number raised, which is what makes the player see an update at all.

.PARAMETER Port
  The loopback port the payload is served on.

.PARAMETER WithRegistry
  Point the uninstall entry at the staged copy for the duration, so that the
  DisplayVersion write is exercised. The entry is exported to a .reg file first
  and restored in a finally block - but a script that is *killed* rather than
  finished never reaches it, so the backup's path is printed as a recovery
  command. (Truncating this script's output pipeline is enough to kill it:
  'Select-Object -First n' stops the upstream command, which is how the entry
  was left pointing at a temporary directory once.)

.EXAMPLE
  bun run tauri build --debug
  powershell -ExecutionPolicy Bypass -File scripts/update-test.ps1 -WithRegistry
#>
param(
  [string]$From,
  [string]$Version,
  [int]$Port = 8099,
  [switch]$WithRegistry,
  [int]$Auto = 0,
  [string]$Play
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

# ASCII only inside quoted strings in this file - see the note at the top of
# pack-windows-update.ps1 for what a dash costs here.

$repo = Split-Path -Parent $PSScriptRoot
$conf = Get-Content (Join-Path $repo 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$product = $conf.productName
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product"

if (-not $Version) {
  $parts = $conf.version.Split('.')
  $parts[2] = [int]$parts[2] + 1
  $Version = $parts -join '.'
}

if (-not $From) {
  $entry = Get-ItemProperty $uninstKey -ErrorAction SilentlyContinue
  if ($entry -and $entry.PSObject.Properties['InstallLocation']) {
    $From = $entry.InstallLocation.Trim('"')
  }
  if (-not $From -or -not (Test-Path -LiteralPath $From)) {
    $From = Join-Path $env:LOCALAPPDATA $product
  }
}
if (-not (Test-Path -LiteralPath $From)) {
  Write-Error "no installation to copy at $From - pass -From"
}

$debugExe = Join-Path $repo 'src-tauri/target/debug/frameplayer.exe'
if (-not (Test-Path -LiteralPath $debugExe)) {
  Write-Error "no debug build at $debugExe - run 'bun run tauri build --debug' first"
}

$work = Join-Path $env:TEMP 'frameplayer-update-test'
$serve = Join-Path $work 'serve'
$install = Join-Path $work $product
New-Item -ItemType Directory -Force -Path $work | Out-Null

# A native command that writes to stderr is turned into a terminating error by
# $ErrorActionPreference = 'Stop' in Windows PowerShell, and bun echoes the
# command it is about to run on stderr before every script - so each of these
# calls would fail on its own banner. The exit code is what actually says
# whether it worked.
function Invoke-Tool {
  # An array, not remaining-arguments: a [Parameter()] attribute turns this
  # into an advanced function, and PowerShell then resolves the tool's own
  # short flags against the common parameters - '-w' became an ambiguous
  # -WarningAction, which only showed up on a run that had no key cached yet.
  param([string[]]$Command)
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    & $Command[0] @($Command[1..($Command.Count - 1)]) 2>&1 | Out-String | Write-Verbose
  } finally {
    $ErrorActionPreference = $previous
  }
  if ($LASTEXITCODE -ne 0) { Write-Error "$($Command -join ' ') failed with $LASTEXITCODE" }
}

# --- a throwaway signing key ------------------------------------------------

$keyFile = Join-Path $work 'test.key'
if (-not (Test-Path -LiteralPath $keyFile)) {
  Push-Location $repo
  # --password= rather than -p '': PowerShell drops an empty string on its
  # way to a native command, and the CLI then reports a missing value.
  try {
    Invoke-Tool @('bun', 'run', 'tauri', 'signer', 'generate', '--ci', '--password=', '--write-keys', $keyFile)
  } finally { Pop-Location }
}
$privateKey = (Get-Content -LiteralPath $keyFile -Raw).Trim()
$publicKey = (Get-Content -LiteralPath "$keyFile.pub" -Raw).Trim()

# --- the payload ------------------------------------------------------------

if (Test-Path -LiteralPath $serve) { Remove-Item -Recurse -Force -LiteralPath $serve }
Push-Location $repo
try {
  & (Join-Path $PSScriptRoot 'pack-windows-update.ps1') `
    -Exe $debugExe -Version $Version -OutDir $serve | Out-Null
  $zip = Join-Path $serve "FramePlayer_${Version}_x64.zip"
  $env:TAURI_SIGNING_PRIVATE_KEY = $privateKey
  # The password goes on the command line and not in the environment, because
  # PowerShell *removes* an environment variable assigned an empty string - and
  # the signer, finding none, prompts for one and waits for ever.
  Invoke-Tool @('bun', 'run', 'tauri', 'signer', 'sign', '--password=', $zip)
} finally { Pop-Location }
$signature = (Get-Content -LiteralPath "$zip.sig" -Raw).Trim()

$base = "http://127.0.0.1:$Port"
@{
  version   = $Version
  notes     = "A local test build.`n`n- **Nothing real.** This release does not exist."
  pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  platforms = @{
    'windows-x86_64-zip' = @{ signature = $signature; url = "$base/FramePlayer_${Version}_x64.zip" }
  }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $serve 'latest.json') -Encoding ascii
# -Encoding ascii, not utf8: 5.1's utf8 writes a byte-order mark, and the
# updater parses the manifest with serde_json, which refuses one outright -
# the check then fails with "expected value at line 1 column 1" and the whole
# test quietly takes the installer path instead.

# --- the staged installation ------------------------------------------------

if (Test-Path -LiteralPath $install) { Remove-Item -Recurse -Force -LiteralPath $install }
Copy-Item -Recurse -LiteralPath $From -Destination $install
Copy-Item -LiteralPath $debugExe -Destination (Join-Path $install 'frameplayer.exe') -Force

# Something for the swap to do: the payload carries the clean copies of these
# two, so they are the ones that get renamed aside and replaced. One of them is
# the running executable and the other is in a subdirectory, which is the pair
# worth exercising.
foreach ($rel in @('frameplayer.exe', 'lua/zoompan.lua')) {
  $target = Join-Path $install ($rel -replace '/', '\')
  # .NET rather than Add-Content: its -Encoding Byte is 5.1-only and 7 wants
  # -AsByteStream instead, and this has to run under both.
  $tail = [byte[]](13, 10, 35, 32, 116, 101, 115, 116)
  $stream = [IO.File]::Open($target, [IO.FileMode]::Append)
  try { $stream.Write($tail, 0, $tail.Length) } finally { $stream.Dispose() }
}

Write-Output "staged installation : $install"
Write-Output "payload             : $zip"
Write-Output "announcing          : $Version (the build reports $($conf.version))"

# --- the uninstall entry ----------------------------------------------------

$regBackup = $null
if ($WithRegistry) {
  if (-not (Test-Path $uninstKey)) {
    Write-Output 'no uninstall entry to point at the staged copy; skipping -WithRegistry'
  } else {
    $regBackup = Join-Path $work 'uninstall-entry.reg'
    & reg export "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product" $regBackup /y | Out-Null
    Set-ItemProperty -Path $uninstKey -Name 'InstallLocation' -Value "`"$install`""
    Write-Output "uninstall entry     : pointed at the staged copy until this script finishes."
    Write-Output "                      If it is killed before then, put the real one back with:"
    Write-Output "                        reg import `"$regBackup`""
  }
}

# --- serve, and run it ------------------------------------------------------

# -ArgumentList refuses an empty array, so the two cases are spelled out once.
function Start-Player {
  param([string]$Dir, [string[]]$PlayerArgs)
  $exe = Join-Path $Dir 'frameplayer.exe'
  if ($PlayerArgs.Count -gt 0) {
    Start-Process -FilePath $exe -WorkingDirectory $Dir -ArgumentList $PlayerArgs
  } else {
    Start-Process -FilePath $exe -WorkingDirectory $Dir
  }
}

$server = Start-Job -ScriptBlock {
  param($dir, $port)
  # Node rather than HttpListener: it needs no URL reservation, and the
  # repository depends on node anyway.
  & node -e "
    const http = require('http'), fs = require('fs'), path = require('path');
    http.createServer((req, res) => {
      const name = path.basename(decodeURIComponent(req.url.split('?')[0]));
      const file = path.join(process.argv[1], name);
      if (!fs.existsSync(file)) { res.writeHead(404).end(); return; }
      res.writeHead(200, { 'content-length': fs.statSync(file).size });
      fs.createReadStream(file).pipe(res);
    }).listen(Number(process.argv[2]), '127.0.0.1');
  " $dir $port
} -ArgumentList $serve, $Port

try {
  Start-Sleep -Milliseconds 700
  $env:FP_UPDATE_ENDPOINT = "$base/latest.json"
  $env:FP_UPDATE_PUBKEY = $publicKey
  $playArgs = @()
  if ($Play) { $playArgs += $Play }
  if ($Auto -gt 0) {
    # No hand on the mouse: the player runs the update itself this many seconds
    # after it starts. Everything but the frontend's resume snapshot.
    $env:FP_UPDATE_AUTO = "$Auto"
    Write-Output ''
    Write-Output "Launching the staged copy; it updates itself after $Auto seconds."
    Start-Player $install $playArgs
    # Long enough for the download, the unpack, the swap and the relaunch.
    Start-Sleep -Seconds ($Auto + 40)
  } else {
    $env:FP_UPDATE_AUTO = $null
    Write-Output ''
    Write-Output 'Launching the staged copy. Open a video, then press the update button in the'
    Write-Output 'title bar. The window should go away and come back on the new files; the'
    Write-Output 'launch after that is the one that sweeps the *.fp-old leftovers.'
    Write-Output ''
    Write-Output 'Press Enter here when you are done.'
    Start-Player $install $playArgs
    [void](Read-Host)
  }
} finally {
  Stop-Job $server -ErrorAction SilentlyContinue | Out-Null
  Remove-Job $server -Force -ErrorAction SilentlyContinue | Out-Null
  if ($regBackup) {
    & reg import $regBackup | Out-Null
    Write-Output "uninstall entry restored from $regBackup"
  }
  Write-Output "what the swap left is under $install - delete $work when you are finished with it"
}

# Downloads every binary SDK the Windows build needs into src-tauri/:
#   lib/     libmpv-2.dll (LGPL build), libmpv-wrapper.dll and the Vulkan loader
#   ffmpeg/  the LGPL shared FFmpeg SDK, for the thumbnail and step sidecars
#   tools/   libclang.dll, needed by bindgen when compiling ffmpeg-sys
#
# Run: powershell -ExecutionPolicy Bypass -File scripts/fetch-libs.ps1 [-Arch x64|arm64]
#
# -Arch is the architecture being built for, x64 unless said otherwise. All
# three directories hold one architecture at a time, so switching means running
# this again. libclang follows -Arch too, although bindgen runs on the build
# machine rather than the target: both architectures are built natively (an
# ARM64 runner builds ARM64), and a cross build would need the host's libclang.

param(
    [ValidateSet('x64', 'arm64')]
    [string]$Arch = 'x64'
)

$ErrorActionPreference = 'Stop'

# Each upstream spells the architecture its own way. Nothing but ASCII inside a
# quoted string in this file: Windows PowerShell 5.1 reads a .ps1 without a
# byte-order mark as the ANSI code page (see pack-windows-update.ps1).
$names = @{
    x64   = @{ wrapper = 'windows-x86_64'; mpv = 'x86_64'; vulkan = 'windows'; ffmpeg = 'win64'; wheel = 'win_amd64'; machine = 0x8664 }
    arm64 = @{ wrapper = 'windows-aarch64'; mpv = 'aarch64'; vulkan = 'warm'; ffmpeg = 'winarm64'; wheel = 'win_arm64'; machine = 0xAA64 }
}[$Arch]

# A pattern that starts matching another architecture's asset would otherwise
# surface as error 193 on somebody's machine, after a release. Reads the COFF
# machine field of a PE file: the 'PE\0\0' signature at the offset stored at
# 0x3C, then two bytes. An ARM64X image (LunarG's ARM64 loader is one) carries
# 0xAA64 there as well.
function Assert-Machine($path) {
    $bytes = [IO.File]::ReadAllBytes($path)
    $pe = [BitConverter]::ToInt32($bytes, 0x3C)
    $machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
    if ($machine -ne $names.machine) {
        throw ('{0} is built for machine 0x{1:X4}, not {2} (0x{3:X4})' -f $path, $machine, $Arch, $names.machine)
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$libDir = Join-Path $repoRoot 'src-tauri\lib'
$tmp = Join-Path $env:TEMP "frameplayer-libs-$([guid]::NewGuid().ToString('n').Substring(0,8))"
New-Item -ItemType Directory -Force $libDir | Out-Null
New-Item -ItemType Directory -Force $tmp | Out-Null

# GitHub allows an **unauthenticated** caller 60 API requests an hour, counted
# per IP — and a hosted runner shares its address with every other build on that
# machine, so the two lookups below routinely meet an allowance somebody else
# has already spent. That is what "API rate limit exceeded for <address>" is,
# and it is the one failure retrying cannot pass: the window is an hour wide and
# the job has minutes. A token raises the limit to 1000 an hour per repository,
# which a release run cannot come close to.
#
# In Actions the token exists with no setup and nothing to configure, so the fix
# is simply to pass it in. Locally there is usually none and 60 an hour is ample
# for one developer, so the header is added when a token is present rather than
# being required — a checkout with no token still fetches.
#
# **Only the API calls carry it, never the asset downloads.** Those redirect to
# a storage host which rejects a request bearing a GitHub Authorization header,
# and PowerShell forwards headers across redirects — so authenticating a
# download would break exactly what works today.
function Get-GitHubHeaders {
    $headers = @{ 'User-Agent' = 'frame-player-fetch-libs' }
    $token = $env:GITHUB_TOKEN
    if (-not $token) { $token = $env:GH_TOKEN }
    if ($token) { $headers['Authorization'] = "Bearer $token" }
    return $headers
}

# Retries what is worth retrying — a 5xx, a dropped connection — and refuses to
# retry what is not. An exhausted allowance answers the same way for the rest of
# the hour, so the useful thing to do with it is name it and say what raises it,
# rather than spend three attempts proving it again.
function Invoke-GitHubApi($url) {
    $headers = Get-GitHubHeaders
    $authed = $headers.ContainsKey('Authorization')
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            return Invoke-RestMethod $url -Headers $headers -UseBasicParsing
        }
        catch {
            $err = $_
            $limited = $false
            $when = 'shortly'
            # Defensive because this runs only on a failure path: a diagnosis
            # that throws would replace the real error with its own.
            try {
                $resp = $err.Exception.Response
                if ($resp) {
                    $status = [int]$resp.StatusCode
                    $left = $resp.Headers['X-RateLimit-Remaining']
                    if (($status -eq 403 -or $status -eq 429) -and $left -eq '0') {
                        $limited = $true
                        $reset = $resp.Headers['X-RateLimit-Reset']
                        if ($reset) {
                            $at = [DateTimeOffset]::FromUnixTimeSeconds([int64]$reset)
                            $when = $at.LocalDateTime.ToString('HH:mm:ss')
                        }
                    }
                }
            }
            catch { }
            if ($limited) {
                if ($authed) {
                    throw "GitHub API rate limit exceeded for $url, and a token was sent - so this is the repository's own 1000/hour allowance. It resets at $when."
                }
                $hint = 'This call was unauthenticated: 60 requests an hour, shared with everything else building from the same address. '
                $hint += 'Set GITHUB_TOKEN to raise it to 1000; in GitHub Actions, pass ${{ secrets.GITHUB_TOKEN }} to the step that runs this script. '
                throw "GitHub API rate limit exceeded for $url. $hint" + "The anonymous allowance resets at $when."
            }
            if ($attempt -eq 3) { throw $err }
            Write-Host "  $url failed (attempt $attempt of 3), retrying..."
            Start-Sleep -Seconds (3 * $attempt)
        }
    }
}

function Get-LatestAsset($repo, $pattern) {
    $release = Invoke-GitHubApi "https://api.github.com/repos/$repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
    if ($null -eq $asset) { throw "Asset matching '$pattern' not found in $repo" }
    return $asset.browser_download_url
}

Write-Host "Fetching the $Arch SDKs."
Write-Host 'Downloading libmpv-wrapper (nini22P/libmpv-wrapper)...'
$wrapperUrl = Get-LatestAsset 'nini22P/libmpv-wrapper' "$($names.wrapper)\.zip$"
$wrapperZip = Join-Path $tmp 'wrapper.zip'
Invoke-WebRequest $wrapperUrl -OutFile $wrapperZip
Expand-Archive $wrapperZip -DestinationPath (Join-Path $tmp 'wrapper')
Assert-Machine (Join-Path $tmp 'wrapper\bin\libmpv-wrapper.dll')
Copy-Item (Join-Path $tmp 'wrapper\bin\libmpv-wrapper.dll') $libDir -Force

Write-Host 'Downloading libmpv-2.dll LGPL build (zhongfly/mpv-winbuild)...'
# Plain x86_64, not the -v3 build: that one requires an AVX2-capable CPU.
$mpvUrl = Get-LatestAsset 'zhongfly/mpv-winbuild' "mpv-dev-lgpl-$($names.mpv)-\d.*\.7z$"
$mpv7z = Join-Path $tmp 'mpvdev.7z'
Invoke-WebRequest $mpvUrl -OutFile $mpv7z
# bsdtar in System32 reads 7z archives.
& "$env:SystemRoot\System32\tar.exe" -xf $mpv7z -C $tmp
Assert-Machine (Join-Path $tmp 'libmpv-2.dll')
Copy-Item (Join-Path $tmp 'libmpv-2.dll') $libDir -Force

Write-Host 'Downloading the Vulkan loader (LunarG Vulkan Runtime)...'
# libmpv-2.dll imports vulkan-1.dll directly, and Windows does not ship it — a
# GPU driver does. Without it libmpv cannot be loaded at all, although mpv draws
# through D3D11 here. The bundled copy is a fallback: libmpv_load.rs uses the
# system's loader whenever there is one, which is why it goes to lib\ and not
# beside the executable, where it would always win. The ARM64 libmpv imports it
# just the same, and LunarG publishes an ARM64 runtime under a platform of its
# own ('warm'), with the loader at the root of the archive rather than under an
# architecture directory.
$vkZip = Join-Path $tmp 'vulkan-rt.zip'
Invoke-WebRequest "https://sdk.lunarg.com/sdk/download/latest/$($names.vulkan)/vulkan-runtime-components.zip" -OutFile $vkZip
$vkDir = Join-Path $tmp 'vulkan-rt'
New-Item -ItemType Directory -Force $vkDir | Out-Null
& "$env:SystemRoot\System32\tar.exe" -xf $vkZip -C $vkDir
# The x64 archive carries an x86 loader as well, hence the directory filter;
# the ARM64 one carries only its own.
$vkDll = Get-ChildItem $vkDir -Recurse -Filter 'vulkan-1.dll' |
    Where-Object { $Arch -ne 'x64' -or $_.Directory.Name -eq 'x64' } | Select-Object -First 1
if ($null -eq $vkDll) { throw "the $Arch vulkan-1.dll was not found in the Vulkan Runtime archive" }
Assert-Machine $vkDll.FullName
# A loader from anywhere but LunarG would be a DLL in every process that plays
# a video, so its Authenticode signature is checked rather than trusted.
$sig = Get-AuthenticodeSignature $vkDll.FullName
if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'LunarG') {
    throw "vulkan-1.dll is not validly signed by LunarG ($($sig.Status): $($sig.SignerCertificate.Subject))"
}
Copy-Item $vkDll.FullName $libDir -Force

Write-Host 'Downloading FFmpeg LGPL shared SDK (BtbN/FFmpeg-Builds, for the sidecars)...'
$ffDir = Join-Path $repoRoot 'src-tauri\ffmpeg'
$ffZip = Join-Path $tmp 'ffmpeg-shared.zip'
Invoke-WebRequest "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-$($names.ffmpeg)-lgpl-shared-8.1.zip" -OutFile $ffZip
& "$env:SystemRoot\System32\tar.exe" -xf $ffZip -C $tmp
$ffSrc = Get-ChildItem $tmp -Directory | Where-Object { $_.Name -like 'ffmpeg-n*shared*' } | Select-Object -First 1
New-Item -ItemType Directory -Force $ffDir | Out-Null
Assert-Machine (Get-ChildItem (Join-Path $ffSrc.FullName 'bin') -Filter 'avcodec-*.dll' | Select-Object -First 1).FullName
foreach ($d in 'bin', 'include', 'lib') {
    Copy-Item (Join-Path $ffSrc.FullName $d) $ffDir -Recurse -Force
}

Write-Host 'Downloading libclang (bindgen needs it to compile ffmpeg-sys)...'
$toolsDir = Join-Path $repoRoot 'src-tauri\tools'
New-Item -ItemType Directory -Force $toolsDir | Out-Null
$whlInfo = Invoke-RestMethod 'https://pypi.org/pypi/libclang/json'
$whlUrl = ($whlInfo.urls | Where-Object { $_.filename -like "*$($names.wheel)*" } | Select-Object -First 1).url
if (-not $whlUrl) { throw "no $($names.wheel) wheel of libclang on PyPI" }
$whl = Join-Path $tmp 'libclang.whl'
Invoke-WebRequest $whlUrl -OutFile $whl
$whlDir = Join-Path $tmp 'libclang'
New-Item -ItemType Directory -Force $whlDir | Out-Null
& "$env:SystemRoot\System32\tar.exe" -xf $whl -C $whlDir
$dll = Get-ChildItem $whlDir -Recurse -Filter 'libclang.dll' | Select-Object -First 1
Assert-Machine $dll.FullName
Copy-Item $dll.FullName $toolsDir -Force

Remove-Item -Recurse -Force $tmp
Write-Host "Done ($Arch)."
Write-Host "  libmpv:  $libDir"
Write-Host "  ffmpeg:  $ffDir"
Write-Host "  tools:   $toolsDir"

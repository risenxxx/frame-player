# Downloads every binary SDK the Windows build needs into src-tauri/:
#   lib/     libmpv-2.dll (LGPL build) and libmpv-wrapper.dll
#   ffmpeg/  the LGPL shared FFmpeg SDK, for the thumbnail and step sidecars
#   tools/   libclang.dll, needed by bindgen when compiling ffmpeg-sys
#
# Run: powershell -ExecutionPolicy Bypass -File scripts/fetch-libs.ps1

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$libDir = Join-Path $repoRoot 'src-tauri\lib'
$tmp = Join-Path $env:TEMP "frameplayer-libs-$([guid]::NewGuid().ToString('n').Substring(0,8))"
New-Item -ItemType Directory -Force $libDir | Out-Null
New-Item -ItemType Directory -Force $tmp | Out-Null

function Get-LatestAsset($repo, $pattern) {
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
    if ($null -eq $asset) { throw "Asset matching '$pattern' not found in $repo" }
    return $asset.browser_download_url
}

Write-Host 'Downloading libmpv-wrapper (nini22P/libmpv-wrapper)...'
$wrapperUrl = Get-LatestAsset 'nini22P/libmpv-wrapper' 'windows-x86_64\.zip$'
$wrapperZip = Join-Path $tmp 'wrapper.zip'
Invoke-WebRequest $wrapperUrl -OutFile $wrapperZip
Expand-Archive $wrapperZip -DestinationPath (Join-Path $tmp 'wrapper')
Copy-Item (Join-Path $tmp 'wrapper\bin\libmpv-wrapper.dll') $libDir -Force

Write-Host 'Downloading libmpv-2.dll LGPL build (zhongfly/mpv-winbuild)...'
# Plain x86_64, not the -v3 build: that one requires an AVX2-capable CPU.
$mpvUrl = Get-LatestAsset 'zhongfly/mpv-winbuild' 'mpv-dev-lgpl-x86_64-\d.*\.7z$'
$mpv7z = Join-Path $tmp 'mpvdev.7z'
Invoke-WebRequest $mpvUrl -OutFile $mpv7z
# bsdtar in System32 reads 7z archives.
& "$env:SystemRoot\System32\tar.exe" -xf $mpv7z -C $tmp
Copy-Item (Join-Path $tmp 'libmpv-2.dll') $libDir -Force

Write-Host 'Downloading FFmpeg LGPL shared SDK (BtbN/FFmpeg-Builds, for the sidecars)...'
$ffDir = Join-Path $repoRoot 'src-tauri\ffmpeg'
$ffZip = Join-Path $tmp 'ffmpeg-shared.zip'
Invoke-WebRequest 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-lgpl-shared-8.1.zip' -OutFile $ffZip
& "$env:SystemRoot\System32\tar.exe" -xf $ffZip -C $tmp
$ffSrc = Get-ChildItem $tmp -Directory | Where-Object { $_.Name -like 'ffmpeg-n*shared*' } | Select-Object -First 1
New-Item -ItemType Directory -Force $ffDir | Out-Null
foreach ($d in 'bin', 'include', 'lib') {
    Copy-Item (Join-Path $ffSrc.FullName $d) $ffDir -Recurse -Force
}

Write-Host 'Downloading libclang (bindgen needs it to compile ffmpeg-sys)...'
$toolsDir = Join-Path $repoRoot 'src-tauri\tools'
New-Item -ItemType Directory -Force $toolsDir | Out-Null
$whlInfo = Invoke-RestMethod 'https://pypi.org/pypi/libclang/json'
$whlUrl = ($whlInfo.urls | Where-Object { $_.filename -like '*win_amd64*' } | Select-Object -First 1).url
$whl = Join-Path $tmp 'libclang.whl'
Invoke-WebRequest $whlUrl -OutFile $whl
$whlDir = Join-Path $tmp 'libclang'
New-Item -ItemType Directory -Force $whlDir | Out-Null
& "$env:SystemRoot\System32\tar.exe" -xf $whl -C $whlDir
$dll = Get-ChildItem $whlDir -Recurse -Filter 'libclang.dll' | Select-Object -First 1
Copy-Item $dll.FullName $toolsDir -Force

Remove-Item -Recurse -Force $tmp
Write-Host "Done."
Write-Host "  libmpv:  $libDir"
Write-Host "  ffmpeg:  $ffDir"
Write-Host "  tools:   $toolsDir"

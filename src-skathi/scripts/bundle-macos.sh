#!/bin/sh
# Lays out version 2's app bundle from a release build, for `vpk pack` to sign and package.
#   src-skathi/scripts/bundle-macos.sh OUT_DIR VERSION
# makes OUT_DIR/Frame Player Beta.app:
#   Contents/MacOS/fpui                   target/release/frameplayer, debug info stripped, its run
#                                         path @executable_path/../Frameworks only
#   Contents/Frameworks/                  src-skathi/lib (scripts/build-macos-libs.sh --v2): libmpv,
#                                         its closure, FFmpeg, the Vulkan loader, MoltenVK
#   Contents/Resources/                   lua/, shaders/, the notices, app.skpk, icon.icns (version
#                                         1's), vulkan/icd.d/MoltenVK_icd.json
# The executable is `fpui`, the name the first betas were installed under, so an update keeps it.
# Nothing is signed for distribution here (vpk signs); edited Mach-O files get an ad-hoc signature.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$here/.." && pwd)
out=$1; version=$2
target=${CARGO_TARGET_DIR:-$here/target}
app="$out/Frame Player Beta.app"
c="$app/Contents"

[ -f "$here/lib/libmpv.dylib" ] || { echo "no $here/lib (scripts/build-macos-libs.sh --v2)" >&2; exit 1; }
[ -f "$here/app/dist/release/.sk/app.skpk" ] || { echo "no payload (scripts/build-macos.sh)" >&2; exit 1; }
rm -rf "$app"
mkdir -p "$c/MacOS" "$c/Frameworks" "$c/Resources/vulkan/icd.d"

cp "$target/release/frameplayer" "$c/MacOS/fpui"
strip -S "$c/MacOS/fpui"
otool -l "$c/MacOS/fpui" | awk '/cmd LC_RPATH/{getline;getline;print $2}' | while read -r rp; do
  case "$rp" in /usr/lib/*) ;; *) install_name_tool -delete_rpath "$rp" "$c/MacOS/fpui" ;; esac
done
install_name_tool -add_rpath @executable_path/../Frameworks "$c/MacOS/fpui"
codesign --force --sign - "$c/MacOS/fpui" 2>/dev/null

cp "$here"/lib/*.dylib "$c/Frameworks/"
# The ICD file names MoltenVK relative to itself.
sed 's#"library_path": *"[^"]*"#"library_path": "../../../Frameworks/libMoltenVK.dylib"#' \
  "$here/lib/MoltenVK_icd.json" > "$c/Resources/vulkan/icd.d/MoltenVK_icd.json"
grep -q '"../../../Frameworks/libMoltenVK.dylib"' "$c/Resources/vulkan/icd.d/MoltenVK_icd.json"

cp -R "$here/resources/lua" "$here/resources/shaders" "$c/Resources/"
cp "$here/resources/LICENSE" "$here/resources/THIRD-PARTY-NOTICES.md" "$c/Resources/"
cp "$here/app/dist/release/.sk/app.skpk" "$c/Resources/"
cp "$repo/src-tauri/icons/icon.icns" "$c/Resources/icon.icns"

cat > "$c/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>app.frameplayer.beta</string>
  <key>CFBundleName</key><string>Frame Player Beta</string>
  <key>CFBundleDisplayName</key><string>Frame Player Beta</string>
  <key>CFBundleExecutable</key><string>fpui</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.entertainment</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Nothing outside the bundle and the system: every non-system reference is @rpath or @loader_path.
for f in "$c/MacOS/fpui" "$c"/Frameworks/*.dylib; do
  if otool -L "$f" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/|@rpath/|@loader_path/)'; then
    echo "$f links something outside the bundle" >&2; exit 1
  fi
done
du -sh "$app"

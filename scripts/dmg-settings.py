# dmgbuild settings for the Frame Player disk image.
#
# This exists because Tauri's own dmg target cannot work on CI: bundle_dmg.sh
# arranges the window by driving Finder over AppleScript, and a headless runner
# has no Finder session — the step ran, reported nothing, and shipped a disk
# image with no .DS_Store at all, which Finder then renders as a plain folder.
# dmgbuild writes the .DS_Store itself, so the layout survives.
#
# Geometry is shared with src-tauri/dmg/background.svg: window_rect's size is
# that image's size, and icon_locations are the centres it was drawn around.
# Change one, change the other.

import os.path

app = defines.get("app", "src-tauri/target/release/bundle/macos/Frame Player.app")
appname = os.path.basename(app)

# UDZO over the better-compressing ULFO: this is the artifact people download
# once, and zlib is readable by every macOS that can run the app.
format = "UDZO"
size = None

files = [app]
symlinks = {"Applications": "/Applications"}
icon = "src-tauri/icons/icon.icns"

background = "src-tauri/dmg/background.tiff"
# Finder's WindowBounds is the window *frame*, not its content: measured on
# macOS 26, the title bar takes 32 pt off the top and a path bar another 28 at
# the bottom. 400 here left 340 pt of content and cut the background off. 432 is
# 400 + title bar, so a viewer without the path bar sees the full 400; the
# artwork keeps everything inside the top 372 for the ones who have it on.
window_rect = ((200, 200), (660, 432))
default_view = "icon-view"

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 180

arrange_by = None
grid_offset = (0, 0)
grid_spacing = 100
scroll_position = (0, 0)
label_pos = "bottom"
text_size = 16
icon_size = 128

icon_locations = {
    appname: (170, 205),
    "Applications": (490, 205),
}

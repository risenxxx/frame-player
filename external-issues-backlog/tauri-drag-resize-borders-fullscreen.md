# [bug] Windows: undecorated window's drag-resize border keeps eating mouse input at the top edge of the screen in fullscreen

**Target repo:** `tauri-apps/tauri`
**Status:** not filed yet
**Our workaround:** `sync_drag_resize_borders()` in `src-tauri/src/lib.rs` (hides the helper window while fullscreen)

## Describe the bug

For undecorated windows (`decorations: false`) tauri-runtime-wry creates an invisible helper child window (class `TAURI_DRAG_RESIZE_BORDERS`) that implements edge resizing. When the parent window is **maximized**, the helper is correctly collapsed to 0×0. But when the parent enters **fullscreen** (`setFullscreen(true)`), the helper stays active.

With default shadows (`shadow: true` → `has_undecorated_shadows == true`) the helper's window region is a strip across the entire top edge, `SM_CYFRAME` tall (~12 physical px at 150% DPI), and its `WM_NCHITTEST` unconditionally returns `HTTOP`. In fullscreen this means the topmost pixels of the screen — the full width, including the area over a custom close button in the top-right corner — never deliver hover/click to the webview. Clicks there attempt a north resize of a fullscreen window, i.e. do nothing, and the cursor shows a ns-resize arrow.

The practical impact: the common muscle-memory gesture "slam the cursor into the top-right corner of the screen and click to close" (Fitts's law) works in a maximized window but silently fails in fullscreen, because the invisible strip sits on top of the close button's top edge.

## Code pointers (tauri-runtime-wry 2.11.4, `src/undecorated_resizing.rs`)

- `subclass_parent` → `WM_SIZE`: collapses the helper only `if is_maximized(parent)`; there is no equivalent branch for fullscreen, so any resize while fullscreen re-applies the full-width top strip region.
- `drag_resize_window_proc` → `WM_NCHITTEST`: `if data.has_undecorated_shadows { return LRESULT(HTTOP as _) }` — the strip always claims the resize border, regardless of the parent being fullscreen.
- For comparison, tao itself handles this state correctly: its `WM_NCCALCSIZE` handler (tao 0.35.3, `src/platform_impl/windows/event_loop.rs`) skips the undecorated-shadow insets when `window_state.fullscreen.is_some()`, so the client area covers the monitor exactly — only the helper window is left over.

## Steps to reproduce

1. Tauri v2 app on Windows, main window with `"decorations": false` (default `shadow: true`), any HTML close button rendered flush to the top-right corner.
2. Enter fullscreen via `getCurrentWindow().setFullscreen(true)`.
3. Move the mouse to the very top edge of the screen (y = 0), e.g. the top-right corner.

**Expected:** the webview receives the events; elements at the top edge (window controls) are hoverable/clickable — same as in the maximized state.

**Actual:** the cursor turns into a ns-resize arrow, hover never reaches the DOM, clicks are swallowed (they post `WM_NCLBUTTONDOWN`/`HTTOP` to a fullscreen parent). `WindowFromPoint` over the top strip returns the `TAURI_DRAG_RESIZE_BORDERS` child instead of the WebView2 window.

Environment: tauri 2.11.5 / tauri-runtime-wry 2.11.4 / tao 0.35.3, Windows 11 24H2, 2560×1440 @ 150% DPI.

## Suggested fix

In `subclass_parent`'s `WM_SIZE` handler, treat fullscreen like maximized and collapse the helper window (or `ShowWindow(child, SW_HIDE)` for the duration of fullscreen). The parent's fullscreen state is available from the runtime; alternatively check that the window rect equals its monitor rect.

## Workaround (app side)

On every `WindowEvent::Resized` of the main window:

```rust
let child = FindWindowExW(hwnd, null, w!("TAURI_DRAG_RESIZE_BORDERS"), w!("TAURI_DRAG_RESIZE_WINDOW"));
ShowWindow(child, if window.is_fullscreen().unwrap_or(false) { SW_HIDE } else { SW_SHOWNOACTIVATE });
```

Hiding survives the helper's own `SetWindowPos` repositioning (it never passes `SWP_SHOWWINDOW`), so the state self-heals on every resize.

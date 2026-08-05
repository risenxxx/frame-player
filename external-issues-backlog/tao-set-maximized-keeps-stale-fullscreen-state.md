# [bug] Windows: `set_maximized` on a fullscreen window leaves `is_fullscreen()` reporting `true` forever

**Target repo:** `tauri-apps/tao`
**Status:** not filed yet
**Our workaround:** the app no longer relies on Tauri's built-in drag-region double-click (`onTitlebarMouseDown` in `src/routes/+page.svelte` owns titlebar drag/maximize and exits fullscreen explicitly)

## Describe the bug

`Window::set_maximized` (tao 0.35.3, `src/platform_impl/windows/window.rs`) only flips the `WindowFlags::MAXIMIZED` flag. If the window is currently in borderless fullscreen, the call visually takes the window out of the fullscreen geometry (it becomes maximized), but `window_state.fullscreen` is never cleared — so `Window::fullscreen()` / Tauri's `isFullscreen()` keep returning `true` for a window that is no longer fullscreen. The bookkeeping stays stale until someone calls `set_fullscreen(None)`.

## How this bites real apps

Tauri's `data-tauri-drag-region` script maximizes on double-click via `internal_toggle_maximize`. If the app also has its own fullscreen mode (video player, F11, double-click on content), a user can end up double-clicking the custom titlebar while the window is fullscreen. The window visibly leaves fullscreen, but any UI state mirrored from `isFullscreen()` (e.g. an enter/exit-fullscreen toggle button) is stuck in the "fullscreen" position, and the app's next `setFullscreen(true)` is a no-op (`old_fullscreen == fullscreen` early-return in `set_fullscreen`).

## Steps to reproduce

1. Undecorated Tauri window with a `data-tauri-drag-region` titlebar.
2. `setFullscreen(true)`.
3. Double-click the titlebar (→ `internal_toggle_maximize` → `set_maximized(true)`).
4. Query `isFullscreen()`.

**Expected:** `false` (the window is maximized, not fullscreen) — or, arguably, `set_maximized` on a fullscreen window should be rejected/exit fullscreen first.

**Actual:** `true`, indefinitely.

Environment: tao 0.35.3 (via tauri 2.11.5), Windows 11 24H2.

## Suggested fix

In `set_maximized`, if `window_state.fullscreen.is_some()`, either clear the fullscreen state through the same path `set_fullscreen(None)` uses (restoring saved window flags) before maximizing, or ignore the call. Mirrors macOS behavior where maximize/zoom during fullscreen is a no-op.

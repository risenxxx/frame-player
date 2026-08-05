use tauri::{PhysicalPosition, WebviewWindow};

/// Minimum pixels of the window that must be visible on some monitor,
/// otherwise the window is considered lost off-screen.
const MIN_VISIBLE: i32 = 100;

/// If the restored window position ended up (almost) entirely off every
/// connected monitor — e.g. a secondary display was unplugged — move the
/// window to the center of the primary monitor instead.
pub fn clamp_to_visible_area(win: &WebviewWindow) {
    let (Ok(pos), Ok(size), Ok(monitors)) = (
        win.outer_position(),
        win.outer_size(),
        win.available_monitors(),
    ) else {
        return;
    };
    if monitors.is_empty() {
        return;
    }

    let (wx, wy) = (pos.x, pos.y);
    let (ww, wh) = (size.width as i32, size.height as i32);

    let visible = monitors.iter().any(|m| {
        let mp = m.position();
        let ms = m.size();
        let ix = (wx + ww).min(mp.x + ms.width as i32) - wx.max(mp.x);
        let iy = (wy + wh).min(mp.y + ms.height as i32) - wy.max(mp.y);
        ix >= MIN_VISIBLE.min(ww) && iy >= MIN_VISIBLE.min(wh)
    });

    if visible {
        return;
    }

    if let Ok(Some(primary)) = win.primary_monitor() {
        let mp = primary.position();
        let ms = primary.size();
        let nx = mp.x + ((ms.width as i32 - ww) / 2).max(0);
        let ny = mp.y + ((ms.height as i32 - wh) / 2).max(0);
        let _ = win.set_position(PhysicalPosition::new(nx, ny));
    }
}

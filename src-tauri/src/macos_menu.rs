//! macOS menu bar.
//!
//! On Windows the whole chrome is drawn in HTML and there is no menu bar at all;
//! on macOS an app without its own menu reads as a port, and some actions are
//! expected to live there. The default Tauri menu gave us Cmd+Q and Cmd+W and
//! nothing else.
//!
//! **The accelerator rule, which is easy to break.** Player hotkeys are handled
//! in the frontend by `e.code` (physical key, works in any layout). Putting an
//! accelerator on a menu item makes the system swallow the keystroke before the
//! webview sees it — so the accelerator does not complement the hotkey, it
//! silently replaces it. Hence only Cmd combinations, which the frontend does
//! not and cannot use (Cmd+O, Cmd+, Cmd+1..3, Cmd+Ctrl+F). Single-key actions
//! (Space, F, L, M, arrows) either appear without an accelerator or not at all —
//! otherwise there would be two sources of truth, diverging at the first edit.
//!
//! **Adding or changing an accelerator here means editing
//! `MAC_MENU_ACCELERATORS` in `src/lib/keys.svelte.ts`.** Hotkeys are editable
//! now, and that set is what stops the editor handing a viewer a combination the
//! system swallows on its way to this menu — a binding that silently never
//! fires, which is the single thing most likely to make the feature look broken.
//! The list there covers AppKit's predefined items (Cmd+Q/W/H/M, the Edit menu)
//! as well as the ones declared below.

use std::sync::{Mutex, Once};

use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, Wry};

/// Webview event; the payload is the id of the clicked item.
pub const MENU_EVENT: &str = "frameplayer://menu";

/// Check marks whose state is owned by the frontend, along with the settings.
pub struct MenuChecks {
    pub remember: CheckMenuItem<Wry>,
    pub fit_to_video: CheckMenuItem<Wry>,
    pub always_on_top: CheckMenuItem<Wry>,
    pub snap_mini: CheckMenuItem<Wry>,
}

/// The items are replaced wholesale when the menu is rebuilt in another
/// language, so they live behind a mutex rather than being managed directly:
/// `manage` keeps the first value it is given, which would leave the check
/// marks pointing at items of a menu that no longer exists.
#[derive(Default)]
pub struct MenuChecksState(Mutex<Option<MenuChecks>>);

/// Menu labels. The menu is built natively, so it cannot reach the frontend
/// dictionary — the two tables here mirror `src/lib/i18n.svelte.ts` and have to
/// be kept in sync by hand. Everything else about the menu is language-agnostic.
struct Strings {
    about: &'static str,
    /// Shown in the standard About panel. LGPL-2.1 section 6 asks for a
    /// prominent notice naming the libraries, and this panel is where a macOS
    /// viewer looks for it.
    ///
    /// It has to be `credits`, not `license`: the standard About panel takes a
    /// fixed set of keys, and muda's macOS path passes only ApplicationName,
    /// ApplicationVersion, Version, Copyright, ApplicationIcon and Credits.
    /// `AboutMetadata.license` exists for the Windows and Linux dialogs and is
    /// silently dropped here — which is exactly how it was found, by the panel
    /// showing nothing but a name and a copyright line.
    credits: &'static str,
    settings: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
    file: &'static str,
    open: &'static str,
    open_link: &'static str,
    reveal: &'static str,
    info: &'static str,
    close_window: &'static str,
    edit: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    paste: &'static str,
    select_all: &'static str,
    view: &'static str,
    fullscreen: &'static str,
    mini: &'static str,
    chapter_prev: &'static str,
    chapter_next: &'static str,
    remember: &'static str,
    fit_to_video: &'static str,
    always_on_top: &'static str,
    snap_mini: &'static str,
    size_50: &'static str,
    size_100: &'static str,
    size_200: &'static str,
    window: &'static str,
    minimize: &'static str,
    maximize: &'static str,
}

const EN: Strings = Strings {
    about: "About Frame Player",
    credits: "Frame Player is free software under the GNU GPL, version 3 or later. It uses mpv, FFmpeg, libplacebo and other libraries under the LGPL and other licenses. Full texts are in LICENSE and THIRD-PARTY-NOTICES.md inside the application.",
    settings: "Settings…",
    services: "Services",
    hide: "Hide Frame Player",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit Frame Player",
    file: "File",
    open: "Open Files…",
    open_link: "Open Location…",
    reveal: "Show in Finder",
    info: "Media Information",
    close_window: "Close Window",
    edit: "Edit",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    paste: "Paste",
    select_all: "Select All",
    view: "View",
    fullscreen: "Full Screen",
    mini: "Mini Player",
    chapter_prev: "Previous Chapter",
    chapter_next: "Next Chapter",
    remember: "Remember Size and Position",
    fit_to_video: "Match Video Aspect Ratio",
    always_on_top: "Always on Top",
    snap_mini: "Snap Mini Player to Edges",
    size_50: "50% of Video Size",
    size_100: "100% of Video Size",
    size_200: "200% of Video Size",
    window: "Window",
    minimize: "Minimize",
    maximize: "Zoom",
};

const RU: Strings = Strings {
    about: "О Frame Player",
    credits: "Frame Player — свободная программа под GNU GPL версии 3 или новее. Он использует mpv, FFmpeg, libplacebo и другие библиотеки — по LGPL и другим лицензиям. Полные тексты в LICENSE и THIRD-PARTY-NOTICES.md внутри приложения.",
    settings: "Параметры…",
    services: "Службы",
    hide: "Скрыть Frame Player",
    hide_others: "Скрыть остальные",
    show_all: "Показать все",
    quit: "Завершить Frame Player",
    file: "Файл",
    open: "Открыть файлы…",
    open_link: "Открыть ссылку…",
    reveal: "Показать в Finder",
    info: "Сведения о файле",
    close_window: "Закрыть окно",
    edit: "Правка",
    undo: "Отменить",
    redo: "Повторить",
    cut: "Вырезать",
    paste: "Вставить",
    select_all: "Выбрать все",
    view: "Вид",
    fullscreen: "Полноэкранный режим",
    mini: "Мини-плеер",
    chapter_prev: "Предыдущая глава",
    chapter_next: "Следующая глава",
    remember: "Запоминать размер и положение",
    fit_to_video: "Подгонять под пропорции видео",
    always_on_top: "Поверх всех окон",
    snap_mini: "Прилипание мини-плеера",
    size_50: "50% размера видео",
    size_100: "100% размера видео",
    size_200: "200% размера видео",
    window: "Окно",
    minimize: "Свернуть",
    maximize: "Развернуть",
};

fn strings(locale: &str) -> &'static Strings {
    if locale.starts_with("ru") {
        &RU
    } else {
        &EN
    }
}

pub fn build(app: &tauri::AppHandle<Wry>, locale: &str) -> tauri::Result<()> {
    let s = strings(locale);
    let pkg = app.package_info().clone();

    let about = PredefinedMenuItem::about(
        app,
        Some(s.about),
        Some(AboutMetadata {
            name: Some("Frame Player".into()),
            version: Some(pkg.version.to_string()),
            copyright: Some("Copyright © 2026 Evgenii Zakharov".into()),
            credits: Some(s.credits.into()),
            ..Default::default()
        }),
    )?;
    let settings = MenuItem::with_id(app, "settings", s.settings, true, Some("CmdOrCtrl+,"))?;
    let app_menu = Submenu::with_items(
        app,
        "Frame Player",
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, Some(s.services))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(s.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(s.hide_others))?,
            &PredefinedMenuItem::show_all(app, Some(s.show_all))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(s.quit))?,
        ],
    )?;

    let open = MenuItem::with_id(app, "open", s.open, true, Some("CmdOrCtrl+O"))?;
    // Cmd+L is the macOS convention for this (QuickTime's "Open Location…").
    let open_link = MenuItem::with_id(app, "open_link", s.open_link, true, Some("CmdOrCtrl+L"))?;
    let reveal = MenuItem::with_id(app, "reveal", s.reveal, true, None::<&str>)?;
    // Cmd+I: the frontend's own binding is a bare `I`, which the system never
    // takes, so this complements it rather than replacing it.
    let info = MenuItem::with_id(app, "info", s.info, true, Some("CmdOrCtrl+I"))?;
    let file_menu = Submenu::with_items(
        app,
        s.file,
        true,
        &[
            &open,
            &open_link,
            &reveal,
            &info,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, Some(s.close_window))?,
        ],
    )?;

    // Without an Edit menu the system never delivers ⌘V, ⌘X or ⌘A to a text
    // field at all — AppKit dispatches those through menu key equivalents, and
    // replacing Tauri's default menu with our own took them away. That is why
    // the link field could not be pasted into.
    //
    // COPY IS DELIBERATELY ABSENT. ⌘C is this player's "copy the current frame",
    // and a menu item would swallow the keystroke before the webview sees it
    // (the accelerator rule at the top of this file). Copying text out of a
    // field still works through the right-click menu, which the frontend now
    // leaves alone over editable elements.
    let edit_menu = Submenu::with_items(
        app,
        s.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(s.undo))?,
            &PredefinedMenuItem::redo(app, Some(s.redo))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(s.cut))?,
            &PredefinedMenuItem::paste(app, Some(s.paste))?,
            &PredefinedMenuItem::select_all(app, Some(s.select_all))?,
        ],
    )?;

    // Cmd+Ctrl+F is the system-wide fullscreen combination on macOS.
    let fullscreen = MenuItem::with_id(
        app,
        "fullscreen",
        s.fullscreen,
        true,
        Some("CmdOrCtrl+Control+F"),
    )?;
    // Chapters, deliberately WITHOUT an accelerator. ⌘⇧←/→ was tried and does
    // nothing: an arrow-key equivalent on an NSMenuItem never fires here, while
    // the same keystroke reaches the webview perfectly well. So the shortcut
    // lives in the frontend as ⌃⌘←/→ (plain Ctrl+arrow being Mission Control),
    // and these items are the discoverable, clickable half of it. Adding an
    // accelerator back would not just be redundant — the system would swallow
    // the keystroke on its way to the webview and break the working binding.
    let chapter_prev = MenuItem::with_id(app, "chapter_prev", s.chapter_prev, true, None::<&str>)?;
    let chapter_next = MenuItem::with_id(app, "chapter_next", s.chapter_next, true, None::<&str>)?;
    // ⌘M is minimize (a predefined item in the Window menu), so the mini player
    // takes ⌘⇧M. The frontend's own binding is a bare `P`, which the system
    // never takes, so the two complement each other.
    let mini = MenuItem::with_id(app, "mini", s.mini, true, Some("CmdOrCtrl+Shift+M"))?;
    let remember =
        CheckMenuItem::with_id(app, "win_remember", s.remember, true, false, None::<&str>)?;
    let fit_to_video =
        CheckMenuItem::with_id(app, "win_fit", s.fit_to_video, true, false, None::<&str>)?;
    let always_on_top =
        CheckMenuItem::with_id(app, "win_ontop", s.always_on_top, true, false, None::<&str>)?;
    let snap_mini =
        CheckMenuItem::with_id(app, "win_snap", s.snap_mini, true, false, None::<&str>)?;
    let size_half = MenuItem::with_id(app, "win_size_50", s.size_50, true, Some("CmdOrCtrl+1"))?;
    let size_one = MenuItem::with_id(app, "win_size_100", s.size_100, true, Some("CmdOrCtrl+2"))?;
    let size_two = MenuItem::with_id(app, "win_size_200", s.size_200, true, Some("CmdOrCtrl+3"))?;
    let view_menu = Submenu::with_items(
        app,
        s.view,
        true,
        &[
            &fullscreen,
            &mini,
            &PredefinedMenuItem::separator(app)?,
            &chapter_prev,
            &chapter_next,
            &PredefinedMenuItem::separator(app)?,
            &remember,
            &fit_to_video,
            &always_on_top,
            &snap_mini,
            &PredefinedMenuItem::separator(app)?,
            &size_half,
            &size_one,
            &size_two,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        s.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(s.minimize))?,
            &PredefinedMenuItem::maximize(app, Some(s.maximize))?,
        ],
    )?;

    let menu = Menu::with_items(app, &[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu])?;
    app.set_menu(menu)?;

    let checks = MenuChecks {
        remember,
        fit_to_video,
        always_on_top,
        snap_mini,
    };
    match app.try_state::<MenuChecksState>() {
        Some(state) => *state.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(checks),
        None => {
            app.manage(MenuChecksState(Mutex::new(Some(checks))));
        }
    }

    // Items do nothing on their own: they forward their id to the frontend,
    // where all player state lives. That way the menu and the context menu run
    // literally the same code. Registered once for the process: handlers stack
    // up, so re-registering on every rebuild would fire each action twice.
    static HANDLER: Once = Once::new();
    HANDLER.call_once(|| {
        app.on_menu_event(|app, event| {
            let _ = app.emit(MENU_EVENT, event.id().0.as_str());
        });
    });

    Ok(())
}

/// Rebuild the menu in another language. The frontend owns the language choice
/// (it is stored in localStorage), so the menu is built in English at startup
/// and corrected from `onMount` — before the window is shown, and long before
/// the app is activated and the menu bar is drawn.
#[tauri::command]
pub fn set_menu_locale(app: tauri::AppHandle<Wry>, locale: String) {
    // muda requires the main thread; commands arrive on a worker.
    let _ = app.clone().run_on_main_thread(move || {
        if let Err(e) = build(&app, &locale) {
            eprintln!("[macos_menu] rebuild failed: {e}");
        }
    });
}

/// Sync the check marks: the frontend owns the settings, the menu only shows them.
#[tauri::command]
pub fn sync_window_menu(
    app: tauri::AppHandle<Wry>,
    remember: bool,
    fit_to_video: bool,
    always_on_top: bool,
    snap_mini: bool,
) {
    // muda requires the main thread; commands arrive on a worker.
    let _ = app.clone().run_on_main_thread(move || {
        let Some(state) = app.try_state::<MenuChecksState>() else {
            return;
        };
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(checks) = guard.as_ref() else {
            return;
        };
        let _ = checks.remember.set_checked(remember);
        let _ = checks.fit_to_video.set_checked(fit_to_video);
        let _ = checks.always_on_top.set_checked(always_on_top);
        let _ = checks.snap_mini.set_checked(snap_mini);
    });
}

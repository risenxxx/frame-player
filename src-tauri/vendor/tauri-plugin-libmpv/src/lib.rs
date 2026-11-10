use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, RunEvent, Runtime, WindowEvent,
};

pub use models::*;

// Vendored: Linux gets its own desktop backend (libmpv's render API, no C
// wrapper, no `wid`) — see src/linux/mod.rs. Windows and macOS are unchanged.
#[cfg(all(desktop, not(target_os = "linux")))]
mod desktop;
#[cfg(target_os = "linux")]
#[path = "linux/mod.rs"]
mod desktop;
#[cfg(mobile)]
mod mobile;
#[cfg(all(desktop, not(target_os = "linux")))]
mod wrapper;

mod commands;
mod error;
mod models;
#[cfg(not(target_os = "linux"))]
mod utils;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::Mpv;
#[cfg(mobile)]
use mobile::Mpv;

pub trait MpvExt<R: Runtime> {
    fn mpv(&self) -> &Mpv<R>;
}

impl<R: Runtime, T: Manager<R>> crate::MpvExt<R> for T {
    fn mpv(&self) -> &Mpv<R> {
        self.state::<Mpv<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("libmpv")
        .invoke_handler(tauri::generate_handler![
            commands::init,
            commands::destroy,
            commands::command,
            commands::set_property,
            commands::get_property,
            commands::set_video_margin_ratio,
        ])
        .setup(|app, api| {
            unsafe {
                let locale = std::ffi::CString::new("C").unwrap();
                libc::setlocale(libc::LC_NUMERIC, locale.as_ptr());
            }

            #[cfg(mobile)]
            let mpv = mobile::init(app, api)?;
            #[cfg(desktop)]
            let mpv = desktop::init(app, api)?;
            app.manage(mpv);
            Ok(())
        })
        .on_event(|app_handle, run_event| {
            // Vendored (Linux): mpv is torn down before the process ends, in
            // the order the render API needs (render context, then core). The
            // window's own destruction has freed the render context already.
            #[cfg(target_os = "linux")]
            if let RunEvent::Exit = run_event {
                app_handle.mpv().destroy_all();
            }
            if let RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } = run_event
            {
                let mpv_state = app_handle.state::<Mpv<R>>();

                let instance_exists = {
                    let instances_lock = match mpv_state.instances.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            log::warn!("Mutex for mpv instances was poisoned. Recovering.");
                            poisoned.into_inner()
                        }
                    };
                    instances_lock.contains_key(label)
                };

                if instance_exists {
                    api.prevent_close();

                    let app_handle_clone = app_handle.clone();
                    let window_label = label.to_string();

                    tauri::async_runtime::spawn(async move {
                        log::info!(
                            "Close requested for '{}', destroying mpv instance first...",
                            &window_label
                        );

                        if let Err(e) = app_handle_clone.mpv().destroy(&window_label) {
                            log::error!(
                                "Failed to destroy mpv for '{}': {}. Still closing.",
                                &window_label,
                                e
                            );
                        }

                        if let Some(window) = app_handle_clone.get_webview_window(&window_label) {
                            if let Err(e) = window.close() {
                                log::error!("Failed to close window '{}': {}", &window_label, e);
                            }
                        }
                    });
                }
            }
        })
        .build()
}

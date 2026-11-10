//! The plugin's desktop backend on Linux (added in this vendored copy; the
//! Windows/macOS backend in `desktop.rs` is the registry's code unchanged).
//!
//! Same public surface as `desktop.rs` — `init`, `destroy`, `command`,
//! `set_property`, `get_property`, `set_video_margin_ratio` and the
//! `instances` map — so the commands, the close handler and the frontend see
//! no difference. Underneath, two things differ:
//!
//! - the C `libmpv-wrapper` is replaced by `client.rs`, which talks to
//!   `libmpv.so.2` directly and reproduces the wrapper's behaviour and JSON,
//!   minus three of its defects (listed there);
//! - the picture is not embedded with `--wid` but drawn through libmpv's
//!   OpenGL render API into a GtkGLArea under the web view (`render.rs`),
//!   with `vo=libmpv`.

mod client;
mod ffi;
mod render;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// The lifecycle lines (render context, teardown, failures) go to stderr with
// eprintln!, not through `log`: the app installs no logger, and it captures
// stderr into its issue report (src-tauri/src/logs.rs). The rest stays `log`,
// as in desktop.rs.
use log::{error, info, trace, warn};
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Emitter, Manager, Runtime};

use crate::models::*;
use crate::Result;

use self::client::{Core, Handle};
use self::ffi::Lib;

/// How long `init` waits for the GTK thread to build the video layer, and
/// `destroy` for it to free the render context. Generous: the GTK thread may
/// be busy for a moment, but never for this long unless it is stuck.
const MAIN_THREAD_WAIT: Duration = Duration::from_secs(10);

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<Mpv<R>> {
    info!("Plugin registered (Linux render-API backend).");
    Ok(Mpv {
        app: app.clone(),
        instances: Mutex::new(HashMap::new()),
        lib: OnceCell::new(),
    })
}

/// One window's mpv.
pub struct MpvInstance {
    core: Core,
    /// Whether a render context was attached (false for audio-only).
    video: bool,
}

pub struct Mpv<R: Runtime> {
    app: AppHandle<R>,
    pub instances: Mutex<HashMap<String, MpvInstance>>,
    lib: OnceCell<Arc<Lib>>,
}

/// Run `f` on the GTK main thread and wait for its answer — inline when
/// already there (waiting on ourselves would never end).
fn on_main_thread<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    if gtk::is_initialized_main_thread() {
        return Some(f());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(MAIN_THREAD_WAIT).ok()
}

impl<R: Runtime> Mpv<R> {
    fn lib(&self) -> Result<Arc<Lib>> {
        self.lib
            .get_or_try_init(|| {
                let lib = Lib::load().map(Arc::new);
                if let Ok(l) = &lib {
                    // SAFETY: a pure query on the loaded library.
                    let v = unsafe { (l.client_api_version)() };
                    eprintln!("[libmpv] loaded, client API {}.{}", v >> 16, v & 0xffff);
                }
                lib
            })
            .cloned()
            .map_err(crate::Error::FFI)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, MpvInstance>> {
        match self.instances.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Mutex was poisoned, recovering.");
                poisoned.into_inner()
            }
        }
    }

    /// Vendored, Linux only: build the window's video layer (the GTK overlay
    /// the picture is drawn in) now, before the page loads, rather than when
    /// the frontend first starts mpv. Call it from the code that creates the
    /// window, on the main thread. Without it `init` builds the layer itself,
    /// late — see `render::prepare` for what that risks.
    pub fn prepare_window(&self, window: &tauri::WebviewWindow<R>) {
        let lib = match self.lib() {
            Ok(lib) => lib,
            Err(e) => {
                eprintln!("[libmpv] No video layer prepared: {e}");
                return;
            }
        };
        let label = window.label().to_string();
        let done = window.with_webview(move |pw| {
            if let Err(e) = render::prepare(&label, &pw.inner(), lib) {
                eprintln!("[libmpv] No video layer prepared for '{label}': {e}");
            }
        });
        if let Err(e) = done {
            eprintln!("[libmpv] No video layer prepared: {e}");
        }
    }

    pub fn init(&self, mpv_config: MpvConfig, window_label: &str) -> Result<String> {
        let lib = self.lib()?;
        if self.lock().contains_key(window_label) {
            info!("mpv instance for window '{window_label}' already exists. Skipping initialization.");
            return Ok(window_label.to_string());
        }

        let mut options = mpv_config.initial_options.clone();
        let audio_only = options.iter().any(|(key, value)| {
            (key == "video" || key == "vid") && (value == "no" || value == false)
        });
        if audio_only {
            info!("Audio-only mode detected for window '{window_label}'. Skipping the video layer.");
        } else {
            // The render API is the only video output that draws into our
            // GL area; any other `vo` (ours elsewhere is gpu-next, an mpv.conf
            // may name one) would open a window of its own or none at all.
            match options.get_mut("vo") {
                Some(v) if v.as_str() != Some("libmpv") => {
                    info!("vo={v} replaced with vo=libmpv (render API) on Linux.");
                    *v = serde_json::json!("libmpv");
                }
                Some(_) => {}
                None => {
                    options.insert("vo".into(), serde_json::json!("libmpv"));
                }
            }
        }

        let mut core = Core::create(lib.clone(), &options, &mpv_config.observed_properties)
            .map_err(crate::Error::CreateInstanceBecause)?;

        if !audio_only {
            let window = match self.app.get_webview_window(window_label) {
                Some(w) => w,
                None => {
                    core.terminate();
                    return Err(crate::Error::WindowNotFound(window_label.to_string()));
                }
            };
            let (tx, rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
            let label = window_label.to_string();
            let main = core.main;
            let gl_lib = lib.clone();
            let sent = window.with_webview(move |pw| {
                let main: Handle = main;
                let _ = tx.send(render::attach(&label, &pw.inner(), gl_lib, main.0));
            });
            let attached = match sent {
                Ok(()) => rx
                    .recv_timeout(MAIN_THREAD_WAIT)
                    .unwrap_or_else(|_| Err("the GTK thread did not answer".into())),
                Err(e) => Err(e.to_string()),
            };
            if let Err(e) = attached {
                eprintln!("[libmpv] No video layer for window '{window_label}': {e}");
                // Nothing to free on the GTK side: attach failed before (or
                // instead of) creating a render context.
                core.terminate();
                return Err(crate::Error::CreateInstanceBecause(e));
            }
        }

        let app = self.app.clone();
        let label = window_label.to_string();
        let event_name = format!("mpv-event-{window_label}");
        core.start_events(move |event| {
            // In order, from this thread: emitting only queues the message for
            // the web view, so it never blocks on the GTK thread.
            if let Err(e) = app.emit_to(&label, &event_name, &event) {
                error!("Failed to emit mpv event to frontend: {e}");
            }
        });

        let mut instances = self.lock();
        if instances.contains_key(window_label) {
            // Somebody else's init won meanwhile (the frontend never races
            // itself, so this is a guard, not a path); ours goes.
            drop(instances);
            warn!("mpv for window '{window_label}' was initialized twice at once.");
            self.teardown(window_label, MpvInstance { core, video: !audio_only });
            return Ok(window_label.to_string());
        }
        instances.insert(window_label.to_string(), MpvInstance { core, video: !audio_only });
        info!("mpv instance initialized for window '{window_label}'.");
        Ok(window_label.to_string())
    }

    /// Render context first (GTK thread, GL current), core second.
    fn teardown(&self, window_label: &str, instance: MpvInstance) {
        if instance.video {
            let label = window_label.to_string();
            if on_main_thread(&self.app, move || render::detach(&label)).is_none() {
                // Destroying a core whose render context is still alive is
                // not allowed; leaking it is the lesser harm.
                eprintln!("[libmpv] Could not free the render context for '{window_label}'; leaving mpv running.");
                std::mem::forget(instance);
                return;
            }
        }
        instance.core.terminate();
    }

    pub fn destroy(&self, window_label: &str) -> Result<()> {
        let removed = self.lock().remove(window_label);
        match removed {
            Some(instance) => {
                self.teardown(window_label, instance);
                eprintln!("[libmpv] mpv for window '{window_label}' destroyed");
            }
            None => trace!("No running mpv instance found for window '{window_label}' to destroy."),
        }
        Ok(())
    }

    /// Every instance, at the end of the process (`RunEvent::Exit`).
    pub fn destroy_all(&self) {
        let labels: Vec<String> = self.lock().keys().cloned().collect();
        for label in labels {
            let _ = self.destroy(&label);
        }
    }

    fn with_instance<T>(
        &self,
        window_label: &str,
        operation: impl FnOnce(&MpvInstance) -> Result<T>,
    ) -> Result<T> {
        let instances = self.lock();
        let instance = instances.get(window_label).ok_or_else(|| {
            crate::Error::InstanceNotFound(format!(
                "mpv instance for window label '{window_label}' not found"
            ))
        })?;
        operation(instance)
    }

    pub fn command(&self, name: &str, args: &Vec<serde_json::Value>, window_label: &str) -> Result<()> {
        if args.is_empty() {
            trace!("COMMAND '{name}'");
        } else {
            trace!("COMMAND '{name}' '{args:?}'");
        }
        self.with_instance(window_label, |i| {
            i.core.command(name, args).map_err(|message| crate::Error::Command {
                window_label: window_label.to_string(),
                message,
            })
        })
    }

    pub fn set_property(&self, name: &str, value: &serde_json::Value, window_label: &str) -> Result<()> {
        trace!("SET PROPERTY '{name}' '{value:?}'");
        self.with_instance(window_label, |i| {
            i.core.set_property(name, value).map_err(|message| crate::Error::SetProperty {
                window_label: window_label.to_string(),
                message,
            })
        })
    }

    pub fn get_property(&self, name: String, format: String, window_label: &str) -> Result<serde_json::Value> {
        self.with_instance(window_label, |i| {
            let err = |message: String| crate::Error::GetProperty {
                window_label: window_label.to_string(),
                message,
            };
            let value = i.core.get_property(&name, &format).map_err(err)?;
            // As desktop.rs: a success without data is an error to the caller.
            if value.is_null() {
                return Err(err("FFI response contained no data".into()));
            }
            trace!("GET PROPERTY '{name}' '{value:?}'");
            Ok(value)
        })
    }

    pub fn set_video_margin_ratio(&self, ratio: VideoMarginRatio, window_label: &str) -> Result<()> {
        trace!("SET VIDEO MARGIN RATIO '{ratio:?}'");
        let margins = [
            ("video-margin-ratio-left", ratio.left),
            ("video-margin-ratio-right", ratio.right),
            ("video-margin-ratio-top", ratio.top),
            ("video-margin-ratio-bottom", ratio.bottom),
        ];
        for (property, value) in margins {
            if let Some(value) = value {
                self.set_property(property, &serde_json::json!(value), window_label)?;
            }
        }
        Ok(())
    }
}

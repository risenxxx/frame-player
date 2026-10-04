//! Which device the system plays sound through right now.
//!
//! Asked for one reason: a Bluetooth speaker adds a delay the system does not
//! report in full, so the picture leads the sound by a fixed amount *for that
//! speaker*, and the player keeps a correction per output device
//! (`audio-output.svelte.ts`). With `audio-device=auto` — the default — mpv
//! plays through the system's default output and says nothing about which one
//! that is; only the system can.
//!
//! The id is the one mpv uses in its own device names, so a device has one key
//! whichever way it was reached: CoreAudio's device UID (`coreaudio/<UID>`)
//! and the WASAPI endpoint id (`wasapi/<id>`).
//!
//! **Polled, not subscribed to.** One thread asks every `POLL` and says so
//! when the answer changes. Both platforms have a change notification, and
//! both would be a callback on a system thread for a question that costs
//! microseconds to ask and changes when somebody pairs a speaker — a second
//! and a half late is not a thing anybody watching a film can see.

use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL: Duration = Duration::from_millis(1500);

#[derive(Clone, PartialEq, Serialize)]
pub struct AudioOutput {
    pub id: String,
    pub name: String,
}

/// The last answer, for the frontend's first question: it may ask before the
/// watcher's first event, and on Windows only the watcher's thread has COM.
static LAST: Mutex<Option<AudioOutput>> = Mutex::new(None);

#[tauri::command]
pub fn audio_output_default() -> Option<AudioOutput> {
    LAST.lock().map(|g| g.clone()).unwrap_or(None)
}

/// Start the watcher. Emits `frameplayer://audio-output` with the new device
/// (or null) whenever the default output changes, the first answer included.
pub fn watch(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let mut source = match platform::Source::new() {
                Some(s) => s,
                None => return,
            };
            let mut first = true;
            loop {
                let now = source.default_output();
                let changed = {
                    let mut last = match LAST.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    let changed = *last != now;
                    *last = now.clone();
                    changed
                };
                if changed || first {
                    let _ = app.emit("frameplayer://audio-output", now);
                    first = false;
                }
                std::thread::sleep(POLL);
            }
        });
    if let Err(e) = spawned {
        eprintln!("[audio-output] could not start the watcher: {e}");
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::AudioOutput;
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    const fn fourcc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const DEFAULT_OUTPUT_DEVICE: u32 = fourcc(b"dOut");
    const DEVICE_UID: u32 = fourcc(b"uid ");
    const OBJECT_NAME: u32 = fourcc(b"lnam");

    pub struct Source;

    impl Source {
        pub fn new() -> Option<Self> {
            Some(Source)
        }

        pub fn default_output(&mut self) -> Option<AudioOutput> {
            let device = default_device()?;
            let id = string_property(device, DEVICE_UID)?;
            let name = string_property(device, OBJECT_NAME).unwrap_or_else(|| id.clone());
            Some(AudioOutput { id, name })
        }
    }

    fn address(selector: u32) -> PropertyAddress {
        PropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
    }

    fn default_device() -> Option<u32> {
        let mut device: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address(DEFAULT_OUTPUT_DEVICE),
                0,
                std::ptr::null(),
                &mut size,
                &mut device as *mut u32 as *mut c_void,
            )
        };
        (status == 0 && device != 0).then_some(device)
    }

    /// Both string properties asked for here hand back a CFString the caller
    /// owns (CoreAudio's "create" rule for them), hence `wrap_under_create_rule`.
    fn string_property(device: u32, selector: u32) -> Option<String> {
        let mut value: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address(selector),
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut CFStringRef as *mut c_void,
            )
        };
        if status != 0 || value.is_null() {
            return None;
        }
        let owned = unsafe { CFString::wrap_under_create_rule(value) };
        let text = owned.to_string();
        (!text.is_empty()).then_some(text)
    }
}

#[cfg(windows)]
mod platform {
    use super::AudioOutput;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };

    /// The enumerator lives on the watcher's thread, which is the one COM was
    /// initialised on.
    pub struct Source {
        enumerator: IMMDeviceEnumerator,
    }

    impl Source {
        pub fn new() -> Option<Self> {
            unsafe {
                // S_FALSE (already initialised) is fine; a mode clash is not
                // ours to fix and the create below will say so.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                Some(Source { enumerator })
            }
        }

        /// `eMultimedia`, the role mpv's WASAPI output asks for when it is
        /// left on its default device.
        pub fn default_output(&mut self) -> Option<AudioOutput> {
            unsafe {
                let device = self.enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
                let raw = device.GetId().ok()?;
                let id = raw.to_string().ok();
                CoTaskMemFree(Some(raw.0 as *const _));
                let id = id?;
                let name = device
                    .OpenPropertyStore(STGM_READ)
                    .ok()
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                    .map(|value| value.to_string())
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| id.clone());
                Some(AudioOutput { id, name })
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::AudioOutput;

    pub struct Source;

    impl Source {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn default_output(&mut self) -> Option<AudioOutput> {
            None
        }
    }
}

//! The player as the system's current media application on macOS: what Control
//! Center shows under Now Playing, and where the keyboard's play/pause key, the
//! Now Playing buttons and the pause that AirPods send when they leave the ears
//! all go.
//!
//! mpv has an integration of its own (`osdep/mac/remote_command_center.swift`)
//! and it is built for mpv.app: its event helper refuses to start unless `NSApp`
//! is mpv's own application class, which under libmpv it never is. What is left
//! then is a listing that never learns a title, a position or a pause — Control
//! Center showed "mpv" with mpv's icon, state "playing" from the first second —
//! and buttons whose presses mpv turns into key events that nothing acts on.
//! `input-media-keys=no` in the player's initial options is what keeps that
//! half-listing off the screen; this file is what takes its place.
//!
//! The frontend owns the facts (`now-playing.svelte.ts`): it sends the title,
//! the length, the position and the rate when any of them changes, and this
//! side only hands them to `MPNowPlayingInfoCenter`. Commands travel the other
//! way as a `frameplayer://remote` event, so that the verbs in `playback` answer
//! them — the same verbs the keys and the buttons use, which is what makes a
//! pause from the AirPods reach a television the player is casting to.
//!
//! The MediaPlayer framework is spoken to dynamically (`msg_send!`) rather than
//! through a bindings crate: six selectors and three constants are not worth a
//! dependency, and every one of them is checked here against the headers.

use tauri::AppHandle;

#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyClass, AnyObject, Bool};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSMutableDictionary, NSNumber, NSObject, NSString};
#[cfg(target_os = "macos")]
use tauri::Emitter;

#[cfg(target_os = "macos")]
#[link(name = "MediaPlayer", kind = "framework")]
extern "C" {
    static MPMediaItemPropertyTitle: &'static NSString;
    static MPMediaItemPropertyPlaybackDuration: &'static NSString;
    static MPNowPlayingInfoPropertyElapsedPlaybackTime: &'static NSString;
    static MPNowPlayingInfoPropertyPlaybackRate: &'static NSString;
    static MPNowPlayingInfoPropertyDefaultPlaybackRate: &'static NSString;
    static MPNowPlayingInfoPropertyMediaType: &'static NSString;
}

/// `MPNowPlayingPlaybackState`.
#[cfg(target_os = "macos")]
const STATE_PLAYING: isize = 1;
#[cfg(target_os = "macos")]
const STATE_PAUSED: isize = 2;
#[cfg(target_os = "macos")]
const STATE_STOPPED: isize = 3;

/// `MPNowPlayingInfoMediaTypeVideo`.
#[cfg(target_os = "macos")]
const MEDIA_TYPE_VIDEO: usize = 2;

/// `MPRemoteCommandHandlerStatusSuccess`.
#[cfg(target_os = "macos")]
const HANDLER_SUCCESS: isize = 0;

/// What the frontend knows about what is playing. `playing` false means there
/// is nothing: the listing is taken down rather than shown stopped.
#[derive(serde::Deserialize)]
pub struct NowPlaying {
    pub playing: bool,
    pub title: String,
    /// Seconds. Zero for a stream whose length is unknown.
    pub duration: f64,
    /// Seconds, as of now: the system extrapolates from it at `rate`.
    pub position: f64,
    /// Playback speed, or 0 while paused.
    pub rate: f64,
    pub paused: bool,
    /// Whether the queue has somewhere to go, which is what shows or hides
    /// the next and previous buttons.
    pub next: bool,
    pub previous: bool,
}

/// A command from the system, as the `frameplayer://remote` event carries it.
#[cfg(target_os = "macos")]
#[derive(serde::Serialize, Clone)]
struct RemoteCommand {
    command: &'static str,
    /// Seconds, for `seek` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<f64>,
}

#[cfg(target_os = "macos")]
fn info_center() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"MPNowPlayingInfoCenter")?;
    Some(unsafe { msg_send![class, defaultCenter] })
}

#[cfg(target_os = "macos")]
fn command_center() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"MPRemoteCommandCenter")?;
    Some(unsafe { msg_send![class, sharedCommandCenter] })
}

/// Main thread only.
#[cfg(target_os = "macos")]
fn apply(info: &NowPlaying) {
    let Some(center) = info_center() else {
        return;
    };
    if !info.playing {
        let none: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            let _: () = msg_send![&*center, setNowPlayingInfo: none];
            let _: () = msg_send![&*center, setPlaybackState: STATE_STOPPED];
        }
        set_enabled(&["nextTrackCommand", "previousTrackCommand"], &[false, false]);
        return;
    }

    let dict = NSMutableDictionary::<NSString, NSObject>::new();
    let number = |v: f64| Retained::into_super(NSNumber::new_f64(v));
    unsafe {
        dict.insert(MPMediaItemPropertyTitle, &*Retained::into_super(NSString::from_str(&info.title)));
        if info.duration > 0.0 {
            dict.insert(MPMediaItemPropertyPlaybackDuration, &*number(info.duration));
        }
        dict.insert(MPNowPlayingInfoPropertyElapsedPlaybackTime, &*number(info.position));
        dict.insert(MPNowPlayingInfoPropertyPlaybackRate, &*number(info.rate));
        dict.insert(MPNowPlayingInfoPropertyDefaultPlaybackRate, &*number(1.0));
        dict.insert(
            MPNowPlayingInfoPropertyMediaType,
            &*Retained::into_super(NSNumber::new_usize(MEDIA_TYPE_VIDEO)),
        );
        let _: () = msg_send![&*center, setNowPlayingInfo: &*dict];
        let state = if info.paused { STATE_PAUSED } else { STATE_PLAYING };
        let _: () = msg_send![&*center, setPlaybackState: state];
    }
    set_enabled(
        &["nextTrackCommand", "previousTrackCommand"],
        &[info.next, info.previous],
    );
}

/// Flip `isEnabled` on the named commands of the shared center. Main thread
/// only. Selectors by name because this is the one place a list beats six
/// lines of the same message.
#[cfg(target_os = "macos")]
fn set_enabled(commands: &[&str], enabled: &[bool]) {
    let Some(center) = command_center() else {
        return;
    };
    for (name, on) in commands.iter().zip(enabled) {
        let Ok(name) = std::ffi::CString::new(*name) else {
            continue;
        };
        let sel = objc2::runtime::Sel::register(&name);
        unsafe {
            let cmd: *mut AnyObject = msg_send![&*center, performSelector: sel];
            if cmd.is_null() {
                continue;
            }
            let _: () = msg_send![cmd, setEnabled: Bool::new(*on)];
        }
    }
}

/// The handlers, installed once. Main thread only.
#[cfg(target_os = "macos")]
fn install(app: AppHandle) {
    let Some(center) = command_center() else {
        return;
    };

    // One handler per command, each a block the command copies and keeps, so
    // nothing here has to outlive the call.
    let handler = |command: &'static str, seek: bool| {
        let app = app.clone();
        block2::RcBlock::new(move |event: core::ptr::NonNull<AnyObject>| -> isize {
            let position = seek.then(|| unsafe {
                let p: f64 = msg_send![event.as_ptr(), positionTime];
                p
            });
            let _ = app.emit("frameplayer://remote", RemoteCommand { command, position });
            HANDLER_SUCCESS
        })
    };
    macro_rules! wire {
        ($sel:ident, $name:literal, $seek:literal) => {{
            let cmd: Retained<AnyObject> = unsafe { msg_send![&*center, $sel] };
            let block = handler($name, $seek);
            let _: *mut AnyObject = unsafe { msg_send![&*cmd, addTargetWithHandler: &*block] };
            unsafe {
                let _: () = msg_send![&*cmd, setEnabled: Bool::YES];
            }
        }};
    }
    wire!(pauseCommand, "pause", false);
    wire!(playCommand, "play", false);
    wire!(togglePlayPauseCommand, "toggle", false);
    wire!(stopCommand, "pause", false);
    wire!(nextTrackCommand, "next", false);
    wire!(previousTrackCommand, "previous", false);
    wire!(changePlaybackPositionCommand, "seek", true);

    // What is not answered is switched off, or Control Center offers it and
    // nothing happens — the skip buttons in particular, which would otherwise
    // replace next and previous.
    set_enabled(
        &[
            "skipForwardCommand",
            "skipBackwardCommand",
            "seekForwardCommand",
            "seekBackwardCommand",
            "changePlaybackRateCommand",
            "changeRepeatModeCommand",
            "changeShuffleModeCommand",
            "ratingCommand",
            "likeCommand",
            "dislikeCommand",
            "bookmarkCommand",
            "enableLanguageOptionCommand",
            "disableLanguageOptionCommand",
        ],
        &[false; 13],
    );
}

#[cfg(target_os = "macos")]
static INSTALLED: std::sync::Once = std::sync::Once::new();

/// Install the command handlers. Called by the frontend once it is listening
/// for `frameplayer://remote`, so that no command is answered into the void;
/// a second call does nothing.
#[tauri::command]
pub fn now_playing_start(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        return app
            .run_on_main_thread(move || INSTALLED.call_once(|| install(handle)))
            .map_err(|e| e.to_string());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

/// What is playing, or that nothing is.
#[tauri::command]
pub fn now_playing_set(app: AppHandle, info: NowPlaying) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return app
            .run_on_main_thread(move || apply(&info))
            .map_err(|e| e.to_string());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, info);
        Ok(())
    }
}

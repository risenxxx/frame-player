//! libmpv's C API, resolved at run time from `libmpv.so.2` — the subset the
//! Linux backend uses: the client API the C wrapper used on the other two
//! platforms, plus the OpenGL render API.
//!
//! Layouts and constants are mpv's `client.h`, `render.h` and `render_gl.h`
//! for client API 2.x (mpv 0.35+). Enumerations are plain integers on purpose:
//! a Rust enum receiving a value it does not list is undefined behaviour, and
//! mpv adds event ids and formats between releases.

// A binding table: it lists what the headers define, not only what is called.
#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr};

pub enum mpv_handle {}
pub enum mpv_render_context {}

pub const MPV_FORMAT_NONE: c_int = 0;
pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_OSD_STRING: c_int = 2;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;
pub const MPV_FORMAT_NODE: c_int = 6;
pub const MPV_FORMAT_NODE_ARRAY: c_int = 7;
pub const MPV_FORMAT_NODE_MAP: c_int = 8;
pub const MPV_FORMAT_BYTE_ARRAY: c_int = 9;

pub const MPV_EVENT_NONE: c_int = 0;
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
pub const MPV_EVENT_LOG_MESSAGE: c_int = 2;
pub const MPV_EVENT_GET_PROPERTY_REPLY: c_int = 3;
pub const MPV_EVENT_SET_PROPERTY_REPLY: c_int = 4;
pub const MPV_EVENT_COMMAND_REPLY: c_int = 5;
pub const MPV_EVENT_START_FILE: c_int = 6;
pub const MPV_EVENT_END_FILE: c_int = 7;
pub const MPV_EVENT_FILE_LOADED: c_int = 8;
pub const MPV_EVENT_IDLE: c_int = 11;
pub const MPV_EVENT_TICK: c_int = 14;
pub const MPV_EVENT_CLIENT_MESSAGE: c_int = 16;
pub const MPV_EVENT_VIDEO_RECONFIG: c_int = 17;
pub const MPV_EVENT_AUDIO_RECONFIG: c_int = 18;
pub const MPV_EVENT_SEEK: c_int = 20;
pub const MPV_EVENT_PLAYBACK_RESTART: c_int = 21;
pub const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;
pub const MPV_EVENT_QUEUE_OVERFLOW: c_int = 24;
pub const MPV_EVENT_HOOK: c_int = 25;

pub const MPV_END_FILE_REASON_EOF: c_int = 0;
pub const MPV_END_FILE_REASON_STOP: c_int = 2;
pub const MPV_END_FILE_REASON_QUIT: c_int = 3;
pub const MPV_END_FILE_REASON_ERROR: c_int = 4;
pub const MPV_END_FILE_REASON_REDIRECT: c_int = 5;

pub type mpv_render_param_type = c_int;
pub const MPV_RENDER_PARAM_INVALID: mpv_render_param_type = 0;
pub const MPV_RENDER_PARAM_API_TYPE: mpv_render_param_type = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: mpv_render_param_type = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: mpv_render_param_type = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: mpv_render_param_type = 4;
pub const MPV_RENDER_PARAM_X11_DISPLAY: mpv_render_param_type = 8;
pub const MPV_RENDER_PARAM_WL_DISPLAY: mpv_render_param_type = 9;
pub const MPV_RENDER_PARAM_ADVANCED_CONTROL: mpv_render_param_type = 10;
pub const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: mpv_render_param_type = 12;

#[repr(C)]
#[derive(Clone, Copy)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut mpv_byte_array,
}

/// 16 bytes on every 64-bit ABI: the union, the format, padding. The C
/// wrapper passed an 8-byte pointer where this belongs — the stack corruption
/// behind "never read a property in `node` format".
#[repr(C)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: c_int,
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_log_message {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
}

#[repr(C)]
pub struct mpv_event_start_file {
    pub playlist_entry_id: i64,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct mpv_event_client_message {
    pub num_args: c_int,
    pub args: *mut *const c_char,
}

#[repr(C)]
pub struct mpv_event_hook {
    pub name: *const c_char,
    pub id: u64,
}

#[repr(C)]
pub struct mpv_event_command {
    pub result: mpv_node,
}

#[repr(C)]
pub struct mpv_render_param {
    pub type_: mpv_render_param_type,
    pub data: *mut c_void,
}

pub type GetProcAddressFn = unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

/// API 2.x: the `extra_exts` field of 1.x is gone.
#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: Option<GetProcAddressFn>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

pub type mpv_render_update_fn = unsafe extern "C" fn(cb_ctx: *mut c_void);

/// libmpv, loaded once per process and never unloaded (it lives in a
/// `OnceCell` of the plugin): every function pointer below stays valid for the
/// life of the process.
pub struct Lib {
    _lib: libloading::Library,
    pub client_api_version: unsafe extern "C" fn() -> std::ffi::c_ulong,
    pub create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub create_client: unsafe extern "C" fn(*mut mpv_handle, *const c_char) -> *mut mpv_handle,
    pub initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub set_option_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub observe_property: unsafe extern "C" fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int,
    pub wait_event: unsafe extern "C" fn(*mut mpv_handle, f64) -> *mut mpv_event,
    pub command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    pub set_property: unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub set_property_string: unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub free_node_contents: unsafe extern "C" fn(*mut mpv_node),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub render_context_create:
        unsafe extern "C" fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int,
    pub render_context_set_update_callback:
        unsafe extern "C" fn(*mut mpv_render_context, Option<mpv_render_update_fn>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(*mut mpv_render_context) -> u64,
    pub render_context_render: unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub render_context_report_swap: unsafe extern "C" fn(*mut mpv_render_context),
    pub render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
}

/// The SONAME, never the unversioned `libmpv.so` (that is the -dev package's
/// link). Distributions that ship mpv ship this.
pub const LIBMPV_SONAME: &str = "libmpv.so.2";

impl Lib {
    pub fn load() -> Result<Lib, String> {
        // SAFETY: loading libmpv runs its ELF constructors, which only
        // initialise its own globals.
        let lib = unsafe { libloading::Library::new(LIBMPV_SONAME) }
            .map_err(|e| format!("could not load {LIBMPV_SONAME}: {e}"))?;
        macro_rules! sym {
            ($name:literal) => {{
                // SAFETY: the type each field is declared with is the C
                // prototype of that symbol in mpv's headers, client API 2.x.
                let s = unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| format!("{LIBMPV_SONAME} has no {}: {e}", $name))?;
                *s
            }};
        }
        Ok(Lib {
            client_api_version: sym!("mpv_client_api_version"),
            create: sym!("mpv_create"),
            create_client: sym!("mpv_create_client"),
            initialize: sym!("mpv_initialize"),
            destroy: sym!("mpv_destroy"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            command: sym!("mpv_command"),
            set_property: sym!("mpv_set_property"),
            set_property_string: sym!("mpv_set_property_string"),
            get_property: sym!("mpv_get_property"),
            free: sym!("mpv_free"),
            free_node_contents: sym!("mpv_free_node_contents"),
            error_string: sym!("mpv_error_string"),
            render_context_create: sym!("mpv_render_context_create"),
            render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
            render_context_update: sym!("mpv_render_context_update"),
            render_context_render: sym!("mpv_render_context_render"),
            render_context_report_swap: sym!("mpv_render_context_report_swap"),
            render_context_free: sym!("mpv_render_context_free"),
            _lib: lib,
        })
    }

    /// mpv's own wording of an error code; never NULL per client.h.
    pub fn error_text(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        unsafe { cstr_lossy((self.error_string)(code)) }
    }
}

/// A C string as Rust text, lossy like the wrapper (`""` for NULL).
///
/// # Safety
/// `p` is NULL or a valid NUL-terminated string.
pub unsafe fn cstr_lossy(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: non-NULL and NUL-terminated per the caller's contract.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

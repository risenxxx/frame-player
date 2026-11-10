//! The mpv client on Linux: what `libmpv-wrapper` does on the other two
//! platforms, in Rust and against libmpv directly.
//!
//! The behaviour is the wrapper's (v0.1.1), operation by operation, because the
//! frontend (`tauri-plugin-libmpv-api`) and the app were written against it:
//! two clients — `main` for every call and `event-client` for observing and
//! waiting; options through `mpv_set_option_string` in the order given, with
//! booleans as `yes`/`no`; observe ids `1..=N` registered before
//! `mpv_initialize`; event objects `{"event": "<kebab-case>", <snake_case
//! fields>}`; every command argument a string; numbers set as DOUBLE. The
//! error texts are the wrapper's too, prefix for prefix.
//!
//! Three of the wrapper's defects are deliberately not reproduced:
//! - `get_property(…, "node")` reads into a real `mpv_node` and frees it with
//!   `mpv_free_node_contents` (the wrapper passed an 8-byte pointer where mpv
//!   writes 16 bytes — the stack corruption the app's rules work around);
//! - a client message is read for `num_args` arguments (the wrapper walked to a
//!   NULL that is not there, so a broadcast `script-message` crashed it);
//! - a failure after `mpv_create` destroys the event client *before*
//!   terminating the core and returns the reason (the wrapper terminated with
//!   the event client still alive, and mpv's shutdown waits for every client:
//!   `init` never returned).

use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::Arc;
use std::thread::JoinHandle;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use super::ffi::{self, cstr_lossy, mpv_handle, Lib};

/// A raw mpv handle that may cross threads: libmpv's client API is
/// thread-safe for every function used on it here.
#[derive(Clone, Copy)]
pub struct Handle(pub *mut mpv_handle);
// SAFETY: see the type's comment — mpv handles are usable from any thread.
unsafe impl Send for Handle {}
// SAFETY: as above.
unsafe impl Sync for Handle {}

pub struct Core {
    pub lib: Arc<Lib>,
    pub main: Handle,
    events: Handle,
    thread: Option<JoinHandle<()>>,
}

fn c_string(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|e| format!("Invalid C-style string provided: {e}"))
}

/// The wrapper's option value conversion (`builder.rs`): booleans as yes/no,
/// numbers as written, strings as they are, anything else skipped.
fn option_text(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(if *b { "yes" } else { "no" }.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// The wrapper's command argument conversion (`ffi.rs`): everything a string;
/// note booleans become `true`/`false`, not yes/no, exactly as there.
fn arg_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string().trim_matches('"').to_string(),
    }
}

fn format_of(name: &str) -> Option<c_int> {
    Some(match name {
        "string" => ffi::MPV_FORMAT_STRING,
        "flag" => ffi::MPV_FORMAT_FLAG,
        "int64" => ffi::MPV_FORMAT_INT64,
        "double" => ffi::MPV_FORMAT_DOUBLE,
        "node" => ffi::MPV_FORMAT_NODE,
        _ => return None,
    })
}

fn number(f: f64) -> Value {
    serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
}

/// An `mpv_node` as JSON, recursively (the wrapper's `MpvNode`, untagged).
///
/// # Safety
/// `node` points to a valid node owned by mpv for the duration of the call.
unsafe fn node_value(node: *const ffi::mpv_node) -> Result<Value, String> {
    // SAFETY: valid per the caller's contract; the union member read is the one
    // `format` names.
    unsafe {
        let n = &*node;
        Ok(match n.format {
            ffi::MPV_FORMAT_NONE => Value::Null,
            ffi::MPV_FORMAT_STRING | ffi::MPV_FORMAT_OSD_STRING => Value::String(cstr_lossy(n.u.string)),
            ffi::MPV_FORMAT_FLAG => Value::Bool(n.u.flag != 0),
            ffi::MPV_FORMAT_INT64 => json!(n.u.int64),
            ffi::MPV_FORMAT_DOUBLE => number(n.u.double_),
            ffi::MPV_FORMAT_NODE_ARRAY => {
                let list = &*n.u.list;
                let mut out = Vec::with_capacity(list.num.max(0) as usize);
                for i in 0..list.num.max(0) as usize {
                    out.push(node_value(list.values.add(i))?);
                }
                Value::Array(out)
            }
            ffi::MPV_FORMAT_NODE_MAP => {
                let list = &*n.u.list;
                let mut out = Map::new();
                for i in 0..list.num.max(0) as usize {
                    out.insert(cstr_lossy(*list.keys.add(i)), node_value(list.values.add(i))?);
                }
                Value::Object(out)
            }
            ffi::MPV_FORMAT_BYTE_ARRAY => {
                let ba = &*n.u.ba;
                let bytes = if ba.data.is_null() {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(ba.data as *const u8, ba.size)
                };
                Value::Array(bytes.iter().map(|b| json!(b)).collect())
            }
            other => return Err(format!("Unsupported mpv_node format: {other}")),
        })
    }
}

/// The data of a property event or reply, by its format (`from_property`).
///
/// # Safety
/// `data` is what mpv delivered with `format`, alive for the call.
unsafe fn property_value(format: c_int, data: *mut c_void) -> Result<Value, String> {
    if data.is_null() {
        return Ok(Value::Null);
    }
    // SAFETY: mpv documents the pointee type for each format.
    unsafe {
        Ok(match format {
            ffi::MPV_FORMAT_NONE => Value::Null,
            ffi::MPV_FORMAT_STRING | ffi::MPV_FORMAT_OSD_STRING => {
                Value::String(cstr_lossy(*(data as *const *const c_char)))
            }
            ffi::MPV_FORMAT_FLAG => Value::Bool(*(data as *const c_int) != 0),
            ffi::MPV_FORMAT_INT64 => json!(*(data as *const i64)),
            ffi::MPV_FORMAT_DOUBLE => number(*(data as *const f64)),
            ffi::MPV_FORMAT_NODE => node_value(data as *const ffi::mpv_node)?,
            other => return Err(format!("Unsupported mpv_node format: {other}")),
        })
    }
}

fn end_file_reason(r: c_int) -> &'static str {
    match r {
        ffi::MPV_END_FILE_REASON_EOF => "eof",
        ffi::MPV_END_FILE_REASON_STOP => "stop",
        ffi::MPV_END_FILE_REASON_QUIT => "quit",
        ffi::MPV_END_FILE_REASON_ERROR => "error",
        ffi::MPV_END_FILE_REASON_REDIRECT => "redirect",
        // The TypeScript type has it; the wrapper could not produce it.
        _ => "unknown",
    }
}

/// One mpv event as the wrapper's JSON, or `None` for one it never forwarded.
///
/// # Safety
/// `ev` is the event `mpv_wait_event` just returned, before the next call.
unsafe fn event_value(ev: &ffi::mpv_event) -> Option<Value> {
    // SAFETY: for each event id mpv documents what `data` points to; every
    // pointer read here is valid until the next mpv_wait_event on this client.
    unsafe {
        let d = ev.data;
        Some(match ev.event_id {
            ffi::MPV_EVENT_SHUTDOWN => json!({"event": "shutdown"}),
            ffi::MPV_EVENT_LOG_MESSAGE => {
                let m = &*(d as *const ffi::mpv_event_log_message);
                json!({"event": "log-message", "prefix": cstr_lossy(m.prefix),
                       "level": cstr_lossy(m.level), "text": cstr_lossy(m.text)})
            }
            ffi::MPV_EVENT_GET_PROPERTY_REPLY => {
                let p = &*(d as *const ffi::mpv_event_property);
                json!({"event": "get-property-reply", "name": cstr_lossy(p.name),
                       "data": property_value(p.format, p.data).ok()?,
                       "error": ev.error, "id": ev.reply_userdata})
            }
            ffi::MPV_EVENT_SET_PROPERTY_REPLY => {
                json!({"event": "set-property-reply", "error": ev.error, "id": ev.reply_userdata})
            }
            ffi::MPV_EVENT_COMMAND_REPLY => {
                let c = &*(d as *const ffi::mpv_event_command);
                json!({"event": "command-reply", "result": node_value(&c.result).ok()?,
                       "error": ev.error, "id": ev.reply_userdata})
            }
            ffi::MPV_EVENT_START_FILE => {
                let s = &*(d as *const ffi::mpv_event_start_file);
                json!({"event": "start-file", "playlist_entry_id": s.playlist_entry_id})
            }
            ffi::MPV_EVENT_END_FILE => {
                let e = &*(d as *const ffi::mpv_event_end_file);
                json!({"event": "end-file", "reason": end_file_reason(e.reason), "error": e.error,
                       "playlist_entry_id": e.playlist_entry_id,
                       "playlist_insert_id": e.playlist_insert_id,
                       "playlist_insert_num_entries": e.playlist_insert_num_entries})
            }
            ffi::MPV_EVENT_FILE_LOADED => json!({"event": "file-loaded"}),
            ffi::MPV_EVENT_IDLE => json!({"event": "idle"}),
            ffi::MPV_EVENT_TICK => json!({"event": "tick"}),
            ffi::MPV_EVENT_CLIENT_MESSAGE => {
                let m = &*(d as *const ffi::mpv_event_client_message);
                // num_args, not a NULL terminator: the array has none.
                let args: Vec<String> = (0..m.num_args.max(0) as usize)
                    .map(|i| cstr_lossy(*m.args.add(i)))
                    .collect();
                json!({"event": "client-message", "args": args})
            }
            ffi::MPV_EVENT_VIDEO_RECONFIG => json!({"event": "video-reconfig"}),
            ffi::MPV_EVENT_AUDIO_RECONFIG => json!({"event": "audio-reconfig"}),
            ffi::MPV_EVENT_SEEK => json!({"event": "seek"}),
            ffi::MPV_EVENT_PLAYBACK_RESTART => json!({"event": "playback-restart"}),
            ffi::MPV_EVENT_PROPERTY_CHANGE => {
                let p = &*(d as *const ffi::mpv_event_property);
                json!({"event": "property-change", "name": cstr_lossy(p.name),
                       "data": property_value(p.format, p.data).ok()?,
                       "id": ev.reply_userdata})
            }
            ffi::MPV_EVENT_QUEUE_OVERFLOW => json!({"event": "queue-overflow"}),
            ffi::MPV_EVENT_HOOK => {
                let h = &*(d as *const ffi::mpv_event_hook);
                json!({"event": "hook", "hook_id": h.id})
            }
            _ => return None,
        })
    }
}

impl Core {
    /// `mpv_wrapper_create` without the event thread, which `start_events`
    /// adds once the caller is ready for it (on Linux: after the render
    /// context exists). On failure every client made so far is gone again and
    /// the error names what mpv refused.
    pub fn create(
        lib: Arc<Lib>,
        options: &IndexMap<String, Value>,
        observed: &IndexMap<String, String>,
    ) -> Result<Core, String> {
        // mpv refuses to create a handle unless LC_NUMERIC is "C", and GTK's
        // init set the locale from the environment. The plugin's setup set it
        // already; this keeps the precondition next to the call that has it.
        // SAFETY: a static, NUL-terminated locale name.
        unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };

        // SAFETY: plain constructor; NULL is checked.
        let main = unsafe { (lib.create)() };
        if main.is_null() {
            return Err("mpv_create failed".into());
        }
        // SAFETY: `main` is a live handle; the name is a static C string.
        let events = unsafe { (lib.create_client)(main, c"event-client".as_ptr()) };
        let core = Core { lib, main: Handle(main), events: Handle(events), thread: None };
        if events.is_null() {
            core.abandon();
            return Err("mpv_create_client failed".into());
        }
        if let Err(e) = core.configure(options, observed) {
            core.abandon();
            return Err(e);
        }
        Ok(core)
    }

    fn configure(
        &self,
        options: &IndexMap<String, Value>,
        observed: &IndexMap<String, String>,
    ) -> Result<(), String> {
        let lib = &self.lib;
        for (name, value) in options {
            let Some(text) = option_text(value) else { continue };
            if name.is_empty() {
                continue;
            }
            let (n, v) = (c_string(name)?, c_string(&text)?);
            // SAFETY: live handle, valid C strings for the call.
            let r = unsafe { (lib.set_option_string)(self.main.0, n.as_ptr(), v.as_ptr()) };
            if r < 0 {
                return Err(format!("Failed to set option '{name}': {}", lib.error_text(r)));
            }
        }
        for (i, (name, format)) in observed.iter().enumerate() {
            let fmt = format_of(format)
                .ok_or_else(|| format!("unknown variant `{format}` for observed property '{name}'"))?;
            let n = c_string(name)?;
            // SAFETY: live handle, valid C string; ids start at 1 as the wrapper's.
            let r = unsafe { (lib.observe_property)(self.events.0, i as u64 + 1, n.as_ptr(), fmt) };
            if r < 0 {
                return Err(format!("Failed to observe property '{name}': {}", lib.error_text(r)));
            }
        }
        // SAFETY: live handle, not yet initialised.
        let r = unsafe { (lib.initialize)(self.main.0) };
        if r < 0 {
            return Err(format!("Failed to initialize mpv: {}", lib.error_text(r)));
        }
        Ok(())
    }

    /// Undo a half-built core: the event client first, or mpv's shutdown waits
    /// for it for ever (the wrapper's hang).
    fn abandon(self) {
        // SAFETY: both handles are live (or NULL) and used by nobody else; no
        // render context was ever created on a core that gets here.
        unsafe {
            if !self.events.0.is_null() {
                (self.lib.destroy)(self.events.0);
            }
            (self.lib.terminate_destroy)(self.main.0);
        }
    }

    /// The wrapper's event loop: one thread waiting on `event-client`, every
    /// event to `emit` in order, until `shutdown` — after which it destroys its
    /// client, which is what lets `terminate` return.
    pub fn start_events(&mut self, emit: impl Fn(Value) + Send + 'static) {
        let lib = self.lib.clone();
        let events = self.events;
        let spawned = std::thread::Builder::new().name("mpv-events".into()).spawn(move || {
            let events = events;
            loop {
                // SAFETY: the event client lives until this thread destroys it.
                let ev = unsafe { &*(lib.wait_event)(events.0, 60.0) };
                if ev.event_id == ffi::MPV_EVENT_NONE {
                    continue;
                }
                // SAFETY: `ev` came from mpv_wait_event just now.
                if let Some(v) = unsafe { event_value(ev) } {
                    emit(v);
                }
                if ev.event_id == ffi::MPV_EVENT_SHUTDOWN {
                    break;
                }
            }
            // SAFETY: nothing else uses the event client.
            unsafe { (lib.destroy)(events.0) };
        });
        match spawned {
            Ok(t) => self.thread = Some(t),
            Err(e) => eprintln!("[libmpv] could not start the mpv event thread: {e}"),
        }
    }

    /// `mpv_terminate_destroy` on `main`, then the event thread joined. Any
    /// render context on this core must already be freed.
    pub fn terminate(mut self) {
        let thread = self.thread.take();
        if thread.is_none() {
            // No thread ever owned the event client.
            self.abandon();
            return;
        }
        // SAFETY: live handle; the caller freed the render context.
        unsafe { (self.lib.terminate_destroy)(self.main.0) };
        if let Some(t) = thread {
            let _ = t.join();
        }
    }

    /// `mpv_wrapper_command`: `Ok(())` or the wrapper's error text.
    pub fn command(&self, name: &str, args: &[Value]) -> Result<(), String> {
        let mut owned = Vec::with_capacity(args.len() + 1);
        owned.push(c_string(name).map_err(|e| format!("mpv command failed: Invalid parameter: {e}"))?);
        for a in args {
            owned.push(c_string(&arg_text(a)).map_err(|e| format!("mpv command failed: Invalid parameter: {e}"))?);
        }
        let mut argv: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        argv.push(std::ptr::null());
        // SAFETY: live handle; a NULL-terminated array of strings alive for the call.
        let r = unsafe { (self.lib.command)(self.main.0, argv.as_mut_ptr()) };
        if r < 0 {
            return Err(format!(
                "mpv command failed: Failed to execute command '{name}': {}",
                self.lib.error_text(r)
            ));
        }
        Ok(())
    }

    /// `mpv_wrapper_set_property`: strings as strings, booleans as FLAG,
    /// every number as DOUBLE (an integer-typed property refuses it, as there).
    pub fn set_property(&self, name: &str, value: &Value) -> Result<(), String> {
        let fail = |r: c_int| {
            format!(
                "Failed to set property: Failed to set property '{name}': {}",
                self.lib.error_text(r)
            )
        };
        let n = c_string(name).map_err(|e| format!("Failed to set property: {e}"))?;
        let r = match value {
            Value::String(s) => {
                let v = c_string(s).map_err(|e| format!("Failed to set property: {e}"))?;
                // SAFETY: live handle, valid C strings.
                unsafe { (self.lib.set_property_string)(self.main.0, n.as_ptr(), v.as_ptr()) }
            }
            Value::Bool(b) => {
                let mut flag: c_int = (*b).into();
                // SAFETY: FLAG takes an int*, alive for the call.
                unsafe {
                    (self.lib.set_property)(self.main.0, n.as_ptr(), ffi::MPV_FORMAT_FLAG, &mut flag as *mut c_int as *mut c_void)
                }
            }
            Value::Number(num) => {
                let mut d = num.as_f64().unwrap_or(0.0);
                // SAFETY: DOUBLE takes a double*, alive for the call.
                unsafe {
                    (self.lib.set_property)(self.main.0, n.as_ptr(), ffi::MPV_FORMAT_DOUBLE, &mut d as *mut f64 as *mut c_void)
                }
            }
            _ => return Err("Unsupported value type for property".into()),
        };
        if r < 0 {
            return Err(fail(r));
        }
        Ok(())
    }

    /// `mpv_wrapper_get_property`, `node` included and correct. An unknown
    /// format name means `string`, as in the wrapper. `Ok(Value::Null)` is a
    /// success without data (a non-finite double), which the caller reports.
    pub fn get_property(&self, name: &str, format: &str) -> Result<Value, String> {
        let fail = |r: c_int| {
            format!(
                "Failed to get property: Failed to get property '{name}': {}",
                self.lib.error_text(r)
            )
        };
        let n = c_string(name).map_err(|e| format!("Failed to get property: {e}"))?;
        let h = self.main.0;
        let get = self.lib.get_property;
        match format_of(format).unwrap_or(ffi::MPV_FORMAT_STRING) {
            ffi::MPV_FORMAT_FLAG => {
                let mut v: c_int = 0;
                // SAFETY: FLAG writes an int.
                let r = unsafe { get(h, n.as_ptr(), ffi::MPV_FORMAT_FLAG, &mut v as *mut c_int as *mut c_void) };
                if r < 0 { Err(fail(r)) } else { Ok(Value::Bool(v != 0)) }
            }
            ffi::MPV_FORMAT_INT64 => {
                let mut v: i64 = 0;
                // SAFETY: INT64 writes an int64_t.
                let r = unsafe { get(h, n.as_ptr(), ffi::MPV_FORMAT_INT64, &mut v as *mut i64 as *mut c_void) };
                if r < 0 { Err(fail(r)) } else { Ok(json!(v)) }
            }
            ffi::MPV_FORMAT_DOUBLE => {
                let mut v: f64 = 0.0;
                // SAFETY: DOUBLE writes a double.
                let r = unsafe { get(h, n.as_ptr(), ffi::MPV_FORMAT_DOUBLE, &mut v as *mut f64 as *mut c_void) };
                if r < 0 { Err(fail(r)) } else { Ok(number(v)) }
            }
            ffi::MPV_FORMAT_NODE => {
                let mut node = ffi::mpv_node { u: ffi::mpv_node_u { int64: 0 }, format: ffi::MPV_FORMAT_NONE };
                // SAFETY: NODE writes a whole mpv_node into `node`, which is one.
                let r = unsafe { get(h, n.as_ptr(), ffi::MPV_FORMAT_NODE, &mut node as *mut ffi::mpv_node as *mut c_void) };
                if r < 0 {
                    return Err(fail(r));
                }
                // SAFETY: mpv filled `node`; it is freed right after the copy.
                let value = unsafe { node_value(&node) };
                // SAFETY: the node's contents were allocated by mpv for us.
                unsafe { (self.lib.free_node_contents)(&mut node) };
                value.map_err(|e| format!("Failed to get property: Failed to convert node data: {e}"))
            }
            _ => {
                let mut p: *mut c_char = std::ptr::null_mut();
                // SAFETY: STRING writes a char* that we free with mpv_free.
                let r = unsafe { get(h, n.as_ptr(), ffi::MPV_FORMAT_STRING, &mut p as *mut *mut c_char as *mut c_void) };
                if r < 0 {
                    return Err(fail(r));
                }
                // SAFETY: NULL or a string mpv allocated for us.
                let s = unsafe { cstr_lossy(p) };
                if !p.is_null() {
                    // SAFETY: as above.
                    unsafe { (self.lib.free)(p as *mut c_void) };
                }
                Ok(Value::String(s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_args_are_stringified_like_the_wrapper() {
        assert_eq!(option_text(&json!(true)).as_deref(), Some("yes"));
        assert_eq!(option_text(&json!(false)).as_deref(), Some("no"));
        assert_eq!(option_text(&json!(90)).as_deref(), Some("90"));
        assert_eq!(option_text(&json!(0.5)).as_deref(), Some("0.5"));
        assert_eq!(option_text(&json!("gpu-next")).as_deref(), Some("gpu-next"));
        assert_eq!(option_text(&Value::Null), None);
        assert_eq!(option_text(&json!([1])), None);

        assert_eq!(arg_text(&json!(true)), "true");
        assert_eq!(arg_text(&json!(30)), "30");
        assert_eq!(arg_text(&json!("a b")), "a b");
        assert_eq!(arg_text(&Value::Null), "null");
        assert_eq!(arg_text(&json!([1, 2])), "[1,2]");
    }

    #[test]
    fn node_conversion_covers_lists_maps_and_nonfinite() {
        let mut key = *b"k\0";
        let mut s = *b"v\0";
        let mut values = [
            ffi::mpv_node { u: ffi::mpv_node_u { string: s.as_mut_ptr() as *mut c_char }, format: ffi::MPV_FORMAT_STRING },
        ];
        let mut keys = [key.as_mut_ptr() as *mut c_char];
        let mut list = ffi::mpv_node_list { num: 1, values: values.as_mut_ptr(), keys: keys.as_mut_ptr() };
        let map = ffi::mpv_node { u: ffi::mpv_node_u { list: &mut list }, format: ffi::MPV_FORMAT_NODE_MAP };
        // SAFETY: every pointer above is to a local alive for the call.
        assert_eq!(unsafe { node_value(&map) }.unwrap(), json!({"k": "v"}));
        let nan = ffi::mpv_node { u: ffi::mpv_node_u { double_: f64::NAN }, format: ffi::MPV_FORMAT_DOUBLE };
        // SAFETY: a local node.
        assert_eq!(unsafe { node_value(&nan) }.unwrap(), Value::Null);
        assert_eq!(end_file_reason(4), "error");
    }

    #[test]
    fn mpv_node_has_the_c_layout() {
        assert_eq!(std::mem::size_of::<ffi::mpv_node>(), 16);
        assert_eq!(std::mem::size_of::<ffi::mpv_event>(), 24);
    }
}

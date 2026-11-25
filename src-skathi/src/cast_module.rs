//! `native:cast`: version 1's device discovery for the cast menu, on both platforms. Google Cast (mDNS and
//! the unicast sweep) and DLNA (SSDP and the sweep) find televisions as version 1 does; nothing past the
//! list is ported (connecting, the LAN server, the session), and the stub says so when a device is picked.
use serde_json::Value;

#[derive(Default)]
pub struct CastModule;

#[skathi::module]
impl CastModule {
    pub fn cast_discover_start(&self, hints: Vec<String>) -> Result<(), String> {
        crate::cast::discover_start(hints)
    }

    pub fn cast_discover_stop(&self) {
        crate::cast::discover_stop()
    }

    pub fn cast_devices(&self) -> Value {
        serde_json::to_value(crate::cast::devices()).unwrap_or(Value::Null)
    }

    pub fn dlna_discover_start(&self, hints: Vec<String>) -> Result<(), String> {
        crate::dlna::discover_start(hints)
    }

    pub fn dlna_discover_stop(&self) {
        crate::dlna::discover_stop()
    }

    pub fn dlna_devices(&self) -> Value {
        serde_json::to_value(crate::dlna::devices()).unwrap_or(Value::Null)
    }
}

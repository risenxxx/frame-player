//! What version 1's `logs.rs` gives the discovery code compiled in here (`crate::logs::*`): the verbose
//! switch (Settings → Report a problem, `commands::debug_log_set`) and `remember`, which in version 1
//! keeps device names out of the issue report. Version 2 has no issue report yet, so there is nothing
//! to keep them out of.
pub fn verbose() -> bool {
    crate::commands::verbose()
}

pub fn remember(_kind: &'static str, _value: &str) {}

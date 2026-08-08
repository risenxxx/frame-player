//! Whether the machine is running on its own battery.
//!
//! Asked by background work that is worth doing while a wall socket is paying
//! for it and not worth doing at the same rate otherwise — today only the
//! thumbnail storyboard, which is the one thing here that can take two cores
//! for the better part of a minute without the viewer asking for anything.
//!
//! Both platforms answer with a single call and no allocation, so the only
//! reason for the cache below is that the storyboard asks after every cell —
//! several times a second on a cheap file.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an answer is reused. Long enough that the question is free, short
/// enough that unplugging a laptop mid-film is noticed within a few cells.
const TTL: Duration = Duration::from_secs(10);

static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// True when the machine is drawing from a battery.
///
/// A desktop always answers false, and so does anything whose power source
/// cannot be read — the fallback is "plugged in", because degrading the
/// player's own behavior on a machine that simply declined to answer is the
/// worse way to be wrong.
pub fn on_battery() -> bool {
    let mut cache = match CACHE.lock() {
        Ok(c) => c,
        Err(p) => p.into_inner(),
    };
    if let Some((at, value)) = *cache {
        if at.elapsed() < TTL {
            return value;
        }
    }
    let value = read();
    *cache = Some((Instant::now(), value));
    value
}

#[cfg(target_os = "macos")]
fn read() -> bool {
    // IOPSGetTimeRemainingEstimate answers kIOPSTimeRemainingUnlimited (-2.0)
    // when something other than a battery is providing the power — which is
    // also what a machine with no battery says. Anything else is a battery:
    // seconds remaining, or kIOPSTimeRemainingUnknown (-1.0) for a battery
    // whose estimate has not settled yet. Verified against `pmset -g batt`.
    //
    // The alternative, IOPSCopyPowerSourcesInfo plus a CFString comparison,
    // needs a release and a toll-free bridge to read one word of meaning.
    const UNLIMITED: f64 = -2.0;
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSGetTimeRemainingEstimate() -> f64;
    }
    unsafe { IOPSGetTimeRemainingEstimate() != UNLIMITED }
}

#[cfg(windows)]
fn read() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    // ACLineStatus: 0 offline, 1 online, 255 unknown. Only an explicit 0 is a
    // battery — a machine that does not know is treated as plugged in.
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return false;
    }
    status.ACLineStatus == 0
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read() -> bool {
    false
}

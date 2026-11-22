#![cfg(windows)]

mod capture;
mod clock;
mod disk;
mod hotkeys;
mod overlay;
mod pixels;
mod record;
mod spool;

pub use capture::ScreenCapturer;
pub use clock::local_time;
pub use disk::free_disk_bytes;
pub use hotkeys::{HotkeyTaken, Hotkeys, KeyCombo};
pub use overlay::{cancel_selection, select_region};
pub use record::record_screen;
pub use spool::Spool;

use gif_core::session::MonitorId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    NoSuchMonitor(MonitorId),
    RegionOnAnotherMonitor { opened: MonitorId, asked: MonitorId },
    EmptyRegion,
    RegionOutsideMonitor { monitor_width: u32, monitor_height: u32 },
    NoFrameYet,
    AccessLost,
    System(String),
}

#[cfg(test)]
mod desktop {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use gif_core::session::MonitorId;

    use crate::ScreenCapturer;

    pub const PRIMARY: MonitorId = MonitorId(0);

    pub static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    pub fn open_primary() -> (MutexGuard<'static, ()>, ScreenCapturer) {
        let alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        (alone, ScreenCapturer::open(PRIMARY).unwrap())
    }
}

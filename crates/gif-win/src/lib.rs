#![cfg(windows)]

mod capture;
mod pixels;
mod record;

pub use capture::ScreenCapturer;
pub use record::record_screen;

use gif_core::session::MonitorId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    NoSuchMonitor(MonitorId),
    RegionOnAnotherMonitor {
        opened: MonitorId,
        asked: MonitorId,
    },
    EmptyRegion,
    RegionOutsideMonitor {
        monitor_width: u32,
        monitor_height: u32,
    },
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

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    pub fn open_primary() -> (MutexGuard<'static, ()>, ScreenCapturer) {
        let alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        (alone, ScreenCapturer::open(PRIMARY).unwrap())
    }
}

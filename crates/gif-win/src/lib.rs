#![cfg(windows)]

mod capture;
mod pixels;

pub use capture::ScreenCapturer;

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

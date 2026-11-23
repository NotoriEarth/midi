use std::time::{Duration, Instant};

use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use windows::Win32::Foundation::{HMODULE, POINT, RECT};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_REMOTESESSION};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_ROTATE90, DXGI_MODE_ROTATION_ROTATE180, DXGI_MODE_ROTATION_ROTATE270,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

use crate::CaptureError;
use crate::pixels::{MonitorImage, Rotation, crop_to_rgb};

const NO_WAIT_MS: u32 = 0;
const FIRST_PICTURE_WAIT: Duration = Duration::from_millis(500);

pub struct ScreenCapturer {
    monitor: MonitorId,
    width: u32,
    height: u32,
    rotation: Rotation,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    latest: ID3D11Texture2D,
    has_picture: bool,
}

impl ScreenCapturer {
    pub fn open(monitor: MonitorId) -> Result<ScreenCapturer, CaptureError> {
        let (adapter, output) = find_monitor(monitor)?;
        let (device, context) = create_device(&adapter)?;

        let output: IDXGIOutput1 = output.cast().map_err(system)?;
        let duplication = unsafe { output.DuplicateOutput(&device) }.map_err(system)?;
        let description = unsafe { duplication.GetDesc() };
        let rotation = match description.Rotation {
            DXGI_MODE_ROTATION_ROTATE90 => Rotation::Clockwise90,
            DXGI_MODE_ROTATION_ROTATE180 => Rotation::Half,
            DXGI_MODE_ROTATION_ROTATE270 => Rotation::Clockwise270,
            _ => Rotation::None,
        };
        let mode = description.ModeDesc;
        let (width, height) = match rotation {
            Rotation::Clockwise90 | Rotation::Clockwise270 => (mode.Height, mode.Width),
            Rotation::None | Rotation::Half => (mode.Width, mode.Height),
        };
        let latest = create_cpu_readable_texture(&device, width, height)?;

        Ok(ScreenCapturer {
            monitor,
            width,
            height,
            rotation,
            context,
            duplication,
            latest,
            has_picture: false,
        })
    }

    pub fn monitor(&self) -> MonitorId {
        self.monitor
    }

    pub fn monitor_size(&self) -> (u32, u32) {
        match self.rotation {
            Rotation::Clockwise90 | Rotation::Clockwise270 => (self.height, self.width),
            Rotation::None | Rotation::Half => (self.width, self.height),
        }
    }

    pub fn capture(&mut self, region: ScreenRect) -> Result<Vec<Rgb>, CaptureError> {
        if region.monitor != self.monitor {
            return Err(CaptureError::RegionOnAnotherMonitor { opened: self.monitor, asked: region.monitor });
        }

        self.copy_new_picture_if_any(NO_WAIT_MS)?;
        let give_up_at = Instant::now() + FIRST_PICTURE_WAIT;
        while !self.has_picture {
            let left = give_up_at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(CaptureError::NoFrameYet);
            }
            self.copy_new_picture_if_any(left.as_millis() as u32)?;
        }
        self.read_latest(region)
    }

    fn copy_new_picture_if_any(&mut self, wait_ms: u32) -> Result<(), CaptureError> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let acquired = unsafe { self.duplication.AcquireNextFrame(wait_ms, &mut info, &mut resource) };

        match acquired {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => return Err(CaptureError::AccessLost),
            Err(error) => return Err(system(error)),
        }

        let desktop_was_redrawn = info.LastPresentTime != 0;
        let copied = if desktop_was_redrawn {
            resource
                .ok_or_else(|| CaptureError::System("the screen frame had no texture".to_string()))
                .and_then(|resource| resource.cast::<ID3D11Texture2D>().map_err(system))
                .map(|screen| unsafe { self.context.CopyResource(&self.latest, &screen) })
        } else {
            Ok(())
        };
        unsafe { self.duplication.ReleaseFrame() }.map_err(system)?;
        copied?;

        self.has_picture |= desktop_was_redrawn;
        Ok(())
    }

    fn read_latest(&self, region: ScreenRect) -> Result<Vec<Rgb>, CaptureError> {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.context.Map(&self.latest, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.map_err(system)?;

        let row_pitch = mapped.RowPitch as usize;
        let bgra = unsafe {
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), row_pitch * self.height as usize)
        };
        let image = MonitorImage { bgra, row_pitch, width: self.width, height: self.height, rotation: self.rotation };
        let pixels = crop_to_rgb(&image, region);

        unsafe { self.context.Unmap(&self.latest, 0) };
        pixels
    }
}

pub(crate) fn system(error: windows::core::Error) -> CaptureError {
    CaptureError::System(error.to_string())
}

fn ends_the_list<T>(result: windows::core::Result<T>) -> Result<Option<T>, CaptureError> {
    match result {
        Ok(found) => Ok(Some(found)),
        Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => Ok(None),
        Err(error) => Err(system(error)),
    }
}

pub fn monitor_size(monitor: MonitorId) -> Result<(u32, u32), CaptureError> {
    let bounds = monitor_bounds(monitor)?;
    Ok(((bounds.right - bounds.left) as u32, (bounds.bottom - bounds.top) as u32))
}

fn monitor_containing((x, y): (i32, i32), monitors: &[RECT]) -> Option<usize> {
    monitors.iter().position(|area| (area.left..area.right).contains(&x) && (area.top..area.bottom).contains(&y))
}

fn every_monitor() -> Result<Vec<RECT>, CaptureError> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(system)?;
    let mut areas = Vec::new();
    for adapter_index in 0.. {
        let Some(adapter) = ends_the_list(unsafe { factory.EnumAdapters1(adapter_index) })? else {
            break;
        };
        for output_index in 0.. {
            let Some(output) = ends_the_list(unsafe { adapter.EnumOutputs(output_index) })? else {
                break;
            };
            areas.push(unsafe { output.GetDesc() }.map_err(system)?.DesktopCoordinates);
        }
    }
    Ok(areas)
}

pub fn in_remote_session() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

pub fn monitor_at_cursor() -> MonitorId {
    let mut cursor = POINT::default();
    let earlier_awareness = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let found = unsafe { GetCursorPos(&mut cursor) }.is_ok();
    unsafe { SetThreadDpiAwarenessContext(earlier_awareness) };
    let monitors = every_monitor().unwrap_or_default();
    let index = found.then(|| monitor_containing((cursor.x, cursor.y), &monitors)).flatten().unwrap_or(0);
    MonitorId(index as u32)
}

pub(crate) fn monitor_bounds(monitor: MonitorId) -> Result<RECT, CaptureError> {
    let (_adapter, output) = find_monitor(monitor)?;
    let description = unsafe { output.GetDesc() }.map_err(system)?;
    Ok(description.DesktopCoordinates)
}

fn find_monitor(monitor: MonitorId) -> Result<(IDXGIAdapter1, IDXGIOutput), CaptureError> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(system)?;
    let mut skipped = 0;

    for adapter_index in 0.. {
        let Some(adapter) = ends_the_list(unsafe { factory.EnumAdapters1(adapter_index) })? else {
            break;
        };
        for output_index in 0.. {
            let Some(output) = ends_the_list(unsafe { adapter.EnumOutputs(output_index) })? else {
                break;
            };
            if skipped == monitor.0 {
                return Ok((adapter, output));
            }
            skipped += 1;
        }
    }
    Err(CaptureError::NoSuchMonitor(monitor))
}

fn create_device(adapter: &IDXGIAdapter1) -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(system)?;

    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(CaptureError::System("Direct3D returned no device".to_string())),
    }
}

fn create_cpu_readable_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, CaptureError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut texture)) }.map_err(system)?;
    texture.ok_or_else(|| CaptureError::System("Direct3D returned no texture".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{PRIMARY, open_primary};

    #[test]
    fn a_point_belongs_to_the_monitor_whose_area_holds_it() {
        let side_by_side = [RECT { left: 0, top: 0, right: 2560, bottom: 1440 }, RECT { left: 2560, top: -155, right: 3640, bottom: 1765 }];
        assert_eq!(monitor_containing((100, 100), &side_by_side), Some(0));
        assert_eq!(monitor_containing((2559, 1439), &side_by_side), Some(0));
        assert_eq!(monitor_containing((2560, 0), &side_by_side), Some(1));
        assert_eq!(monitor_containing((3000, -100), &side_by_side), Some(1));
        assert_eq!(monitor_containing((3640, 0), &side_by_side), None, "the right edge belongs to no monitor");
        assert_eq!(monitor_containing((100, 1500), &side_by_side), None, "below the first monitor");
    }

    #[test]
    #[ignore = "needs an unlocked desktop with a second monitor; run with --ignored"]
    fn a_turned_second_monitor_is_read_the_right_way_round() {
        let _alone = crate::desktop::ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let second = MonitorId(1);
        let Ok(bounds) = monitor_bounds(second) else {
            return;
        };
        let earlier = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let patch = crate::overlay::tests::white_patch(bounds.left + 100, bounds.top + 200, 300, 150);
        unsafe { SetThreadDpiAwarenessContext(earlier) };
        std::thread::sleep(Duration::from_millis(300));

        let mut capturer = ScreenCapturer::open(second).unwrap();
        let (width, height) = capturer.monitor_size();
        let inside_the_patch = capturer.capture(region(second, 110, 210, 280, 130));
        let whole = capturer.capture(region(second, 0, 0, width, height)).map(|pixels| pixels.len());
        unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(patch) }.unwrap();

        assert_eq!((width as i32, height as i32), (bounds.right - bounds.left, bounds.bottom - bounds.top));
        assert_eq!(whole, Ok((width * height) as usize));
        let darkest = *inside_the_patch.unwrap().iter().flatten().min().unwrap();
        assert_eq!(darkest, 255, "the white window was not where the capture looked");
    }

    fn region(monitor: MonitorId, x: i32, y: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect { monitor, x, y, width, height }
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_primary_monitor_reports_a_real_size() {
        let (_alone, capturer) = open_primary();
        let (width, height) = capturer.monitor_size();
        assert!(width >= 640 && height >= 480, "{width}x{height}");
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_region_comes_back_with_one_color_per_pixel() {
        let (_alone, mut capturer) = open_primary();
        let pixels = capturer.capture(region(PRIMARY, 0, 0, 64, 48)).unwrap();
        assert_eq!(pixels.len(), 64 * 48);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn capturing_again_while_the_screen_is_still_gives_the_same_size_picture() {
        let (_alone, mut capturer) = open_primary();
        let whole = {
            let (width, height) = capturer.monitor_size();
            region(PRIMARY, 0, 0, width, height)
        };
        let first = capturer.capture(whole).unwrap();
        for _ in 0..5 {
            assert_eq!(capturer.capture(whole).unwrap().len(), first.len());
        }
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_first_capture_already_shows_the_desktop() {
        let (_alone, mut capturer) = open_primary();
        let (width, height) = capturer.monitor_size();
        let pixels = capturer.capture(region(PRIMARY, 0, 0, width, height)).unwrap();
        let first = pixels[0];
        assert!(pixels.iter().any(|&pixel| pixel != first), "every pixel is {first:?}");
    }

    #[test]
    fn a_monitor_that_is_not_there_is_reported() {
        assert_eq!(ScreenCapturer::open(MonitorId(99)).err(), Some(CaptureError::NoSuchMonitor(MonitorId(99))));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_region_for_another_monitor_is_rejected() {
        let (_alone, mut capturer) = open_primary();
        assert_eq!(
            capturer.capture(region(MonitorId(1), 0, 0, 10, 10)),
            Err(CaptureError::RegionOnAnotherMonitor { opened: PRIMARY, asked: MonitorId(1) })
        );
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_region_larger_than_the_monitor_is_rejected() {
        let (_alone, mut capturer) = open_primary();
        let (monitor_width, monitor_height) = capturer.monitor_size();
        assert_eq!(
            capturer.capture(region(PRIMARY, 0, 0, monitor_width + 1, 10)),
            Err(CaptureError::RegionOutsideMonitor { monitor_width, monitor_height })
        );
    }
}

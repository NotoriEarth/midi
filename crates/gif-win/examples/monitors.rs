use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

fn main() {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.expect("DXGI is available");
    for adapter_index in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(adapter_index) }) else {
            break;
        };
        let description = unsafe { adapter.GetDesc1() }.expect("an adapter has a description");
        let name = String::from_utf16_lossy(&description.Description);
        println!("adapter {adapter_index}: {}", name.trim_end_matches('\0'));

        for output_index in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(output_index) }) else {
                break;
            };
            let description = unsafe { output.GetDesc() }.expect("an output has a description");
            let area = description.DesktopCoordinates;
            println!(
                "  output {output_index}: {}x{} at ({}, {}), attached to desktop: {}",
                area.right - area.left,
                area.bottom - area.top,
                area.left,
                area.top,
                description.AttachedToDesktop.as_bool()
            );
        }
    }
}

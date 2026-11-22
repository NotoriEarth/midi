use gif_core::session::MonitorId;
use gif_win::select_region;

fn main() {
    println!("drag a rectangle on the primary monitor (Esc or right click cancels)");
    match select_region(MonitorId(0)).expect("the primary monitor can show the overlay") {
        Some(region) => println!("selected {}x{} at ({}, {})", region.width, region.height, region.x, region.y),
        None => println!("cancelled"),
    }
}

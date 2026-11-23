use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use gif_core::palette::{ColorCounts, Palette};
use gif_core::writer::Rgb;

struct Measured;

static IN_USE: AtomicUsize = AtomicUsize::new(0);
static MOST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Measured {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let in_use = IN_USE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
        MOST.fetch_max(in_use, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        IN_USE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static MEASURED: Measured = Measured;

const BYTES_PER_LISTED_COLOR: usize = 8;

#[test]
fn choosing_a_palette_needs_no_more_memory_than_one_list_of_its_colors() {
    let distinct = 1 << 20;
    let pixels: Vec<Rgb> = (0..distinct).map(|n: usize| [(n >> 12) as u8, (n >> 4) as u8, (n << 4) as u8]).collect();
    let mut counts = ColorCounts::new();
    counts.add(&pixels);
    drop(pixels);

    let before = IN_USE.load(Ordering::SeqCst);
    MOST.store(before, Ordering::SeqCst);
    let palette = Palette::from_counts(&counts, 255).unwrap();
    let most_added = MOST.load(Ordering::SeqCst) - before;

    assert_eq!(palette.colors().len(), 255);
    let one_list = distinct * BYTES_PER_LISTED_COLOR;
    let a_little_more = 64 * 1024;
    assert!(most_added <= one_list + a_little_more, "{most_added} bytes at most, one list is {one_list}");
}

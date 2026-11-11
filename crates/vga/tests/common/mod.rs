#![allow(dead_code)]

use relocade_machine::{Clock, Device};
use std::time::Duration;

pub fn crtc(device: &mut impl Device, index: u8, value: u8) {
    relocade_machine::IoBus::default().with_context(|mut context| {
        device.write_port(
            context.reborrow(),
            0x3d4,
            2,
            u32::from(u16::from_le_bytes([index, value])),
        );
    })
}

pub fn dots(count: u64, frequency: u64) -> Duration {
    Duration::from_nanos((u128::from(count) * 1_000_000_000).div_ceil(u128::from(frequency)) as u64)
}

// Default hardware timing: 800 dots/line, 449 lines/frame, retrace at line 412.
pub fn boundary(frame: u64) -> Duration {
    dots((412 + (frame - 1) * 449) * 800, 25_175_000)
}

// Elapsed time since entering the fixed 640x480 DISPI mode.
pub fn svga_boundary(frame: u64) -> Duration {
    dots((490 + (frame - 1) * 525) * 800, 25_175_000)
}

// Service pending display work and register its next wait.
pub fn poll_video(
    video: std::pin::Pin<&mut impl std::future::Future<Output = ()>>,
    cx: &mut std::task::Context<'_>,
) {
    assert!(video.poll(cx).is_pending());
}

pub fn service_video(video: std::pin::Pin<&mut impl std::future::Future<Output = ()>>) {
    poll_video(
        video,
        &mut std::task::Context::from_waker(std::task::Waker::noop()),
    );
}

pub fn start_video(
    vga: relocade_vga::Vga,
) -> (
    std::rc::Rc<std::cell::RefCell<relocade_vga::Vga>>,
    std::pin::Pin<Box<impl std::future::Future<Output = ()>>>,
) {
    let vga = std::rc::Rc::new(std::cell::RefCell::new(vga));
    let mut video = Box::pin(relocade_vga::Vga::run(vga.clone()));
    service_video(video.as_mut());
    (vga, video)
}

pub fn snapshot(
    vga: &std::rc::Rc<std::cell::RefCell<relocade_vga::Vga>>,
) -> relocade_vga::DisplaySnapshot {
    vga.borrow().snapshot()
}

#[path = "../support/mod.rs"]
pub mod support;
pub use support::fixture;

pub fn text_vga(clock: Clock) -> relocade_vga::Vga {
    let mut vga = relocade_vga::Vga::with_clock(clock);
    fixture::initialize_font(&mut vga);
    vga
}

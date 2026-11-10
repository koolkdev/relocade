use super::{common, pixel, read_pixels, set_cell};
use futures_util::FutureExt;
use relocade_clock_test_support::ManualClock;
use relocade_frontend::display::DisplayRenderer;
use relocade_machine::{Clock, ClockSource, Device, Instant, Sleep};
use relocade_vga::{registers::crtc, VgaDisplay};
use std::{cell::Cell, rc::Rc, time::Duration};

struct MappedClock {
    device: ManualClock,
    host: ManualClock,
    offset: Rc<Cell<Duration>>,
}

impl ClockSource for MappedClock {
    fn now(&self) -> Instant {
        self.device.now()
    }

    fn sleep_until(&self, deadline: Instant) -> Sleep {
        self.device.sleep_until(deadline)
    }

    fn sync_to_host(&self, time: Instant) -> Sleep {
        self.host
            .sleep_until(self.host.at(self.device.elapsed(time) + self.offset.get()))
    }
}

async fn draw(display: &mut VgaDisplay, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let image = display.encode_image(queue, &mut encoder).unwrap().clone();
    queue.submit([encoder.finish()]);
    read_pixels(device, queue, image.texture()).await
}

async fn timed_updates() {
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let guest = ManualClock::new();
    let host = ManualClock::new();
    let offset = Rc::new(Cell::new(Duration::ZERO));
    let (vga, mut video) = common::start_video(common::text_vga(Clock::new(MappedClock {
        device: guest.clone(),
        host: host.clone(),
        offset: offset.clone(),
    })));
    common::crtc(&mut *vga.borrow_mut(), crtc::CURSOR_START, 0x20);
    set_cell(&mut *vga.borrow_mut(), 0, 0xdb, 4);
    let source = vga.borrow().frames();
    let mut display = VgaDisplay::new(&device, source.clone()).unwrap();

    // Late service keeps the deadline at the retrace boundary.
    guest.advance_to(guest.at(common::boundary(1) + Duration::from_micros(1)));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert!(source.is_empty());
    assert_eq!(host.next_deadline(), Some(host.at(common::boundary(1))));
    // Redraw and cancellation of display service do not affect the producer wait.
    assert!(display.service(&queue).now_or_never().is_none());
    let mut encoder = device.create_command_encoder(&Default::default());
    assert!(display.encode_image(&queue, &mut encoder).is_none());
    queue.submit([encoder.finish()]);

    set_cell(&mut *vga.borrow_mut(), 1, 0xdb, 1);
    guest.advance_to(guest.at(common::boundary(2)));
    vga.borrow_mut().read_port(0x3da, 1);
    // A different memory page distinguishes retaining both updates from only
    // uploading the last publication's dirty pages.
    set_cell(&mut *vga.borrow_mut(), 1999, 0xdb, 2);
    guest.advance_to(guest.at(common::boundary(3)));
    vga.borrow_mut().read_port(0x3da, 1);
    // Pending timestamps use the mapping when the task processes them. An
    // already-started wait keeps its original deadline across mapping changes.
    offset.set(Duration::from_secs(1));
    common::service_video(video.as_mut());
    assert!(source.is_empty());
    assert_eq!(host.next_deadline(), Some(host.at(common::boundary(1))));

    host.advance_to(host.at(common::boundary(1)));
    common::service_video(video.as_mut());

    assert_eq!(
        host.next_deadline(),
        Some(host.at(common::boundary(3) + offset.get()))
    );
    assert_eq!(display.service(&queue).await, Some(true));
    let pixels = draw(&mut display, &device, &queue).await;

    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 0), [0, 0, 0, 255]);

    // The producer coalesces captures two and three while retaining both dirty
    // pages. The graphics worker keeps using the last published capture.
    assert!(display.service(&queue).now_or_never().is_none());
    host.advance_to(host.at(common::boundary(2)));
    common::service_video(video.as_mut());

    assert!(display.service(&queue).now_or_never().is_none());
    assert_eq!(draw(&mut display, &device, &queue).await, pixels);

    display.device_lost();
    display.attach_device(&device, &queue).unwrap();
    assert_eq!(draw(&mut display, &device, &queue).await, pixels);

    assert!(display.service(&queue).now_or_never().is_none());

    host.advance_to(host.at(common::boundary(3) + offset.get()));
    common::service_video(video.as_mut());
    assert_eq!(display.service(&queue).await, Some(true));
    let pixels = draw(&mut display, &device, &queue).await;

    assert_eq!(pixel(&pixels, 0, 0), [170, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 8, 0), [0, 0, 170, 255]);
    assert_eq!(pixel(&pixels, 632, 192), [0, 170, 0, 255]);

    set_cell(&mut *vga.borrow_mut(), 2, 0xdb, 4);
    guest.advance_to(guest.at(common::boundary(4)));
    vga.borrow_mut().read_port(0x3da, 1);
    common::service_video(video.as_mut());
    assert_eq!(
        host.next_deadline(),
        Some(host.at(common::boundary(4) + offset.get()))
    );
    drop(video);
    assert_eq!(host.next_deadline(), None);
    drop(vga);

    assert_eq!(display.service(&queue).await, None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn producer_sync_preserves_redraws_pending_captures_and_recovery() {
    pollster::block_on(timed_updates());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn producer_sync_preserves_redraws_pending_captures_and_recovery() {
    timed_updates().await;
}

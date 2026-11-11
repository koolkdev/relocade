use relocade_clock_test_support::ManualClock;
mod common;
use common::{boundary, crtc};
use relocade_machine::Device;
use relocade_vga::Vga;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn start(vga: &mut impl Device, address: u16) {
    let [high, low] = address.to_be_bytes();
    crtc(vga, 12, high);
    crtc(vga, 13, low);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn page_writes_before_at_and_after_retrace_select_the_following_display() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        for offset in [-1, 0, 1] {
            let clock = ManualClock::default();
            let (vga, mut video) = common::start_video(Vga::with_clock(clock.clock()));
            let source = vga.borrow().frames();
            vga.borrow_mut()
                .write_mmio(context.reborrow(), 0xb8000, 2, 0x0441);
            vga.borrow_mut()
                .write_mmio(context.reborrow(), 0xb8200, 2, 0x0142);
            let change_time = match offset {
                -1 => boundary(1) - Duration::from_nanos(1),
                0 => boundary(1),
                _ => boundary(1) + Duration::from_nanos(1),
            };
            clock.advance_to(clock.at(change_time));
            start(&mut *vga.borrow_mut(), 0x100);
            assert_eq!(vga.borrow().snapshot().vga().unwrap().display_start, 0);
            vga.borrow_mut()
                .write_port(context.reborrow(), 0x3d4, 1, 12);
            assert_eq!(vga.borrow_mut().read_port(context.reborrow(), 0x3d5, 1), 1); // Programmed readback is immediate.

            if offset < 0 {
                clock.advance_to(clock.at(boundary(1)));
                vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
            }
            assert!(source.is_empty());
            common::service_video(video.as_mut());
            let first = source.try_recv().unwrap();
            let first_text = common::support::inspection::inspect(&first);

            assert_eq!(first_text.start_address, if offset < 0 { 0x100 } else { 0 });
            assert_eq!(
                first_text.cells[0].character,
                if offset < 0 { b'B' } else { b'A' }
            );

            clock.advance_to(clock.at(boundary(2)));
            vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
            common::service_video(video.as_mut());
            if offset < 0 {
                assert!(source.is_empty()); // The first snapshot already has this latch.
            } else {
                let second = source.try_recv().unwrap();
                assert_eq!(second.vga().unwrap().display_start, 0x100);
            }
            assert_eq!(first.vga().unwrap().display_start, first_text.start_address);
        }
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn separate_start_bytes_can_straddle_the_latch_edge() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = common::start_video(Vga::with_clock(clock.clock()));
        let source = vga.borrow().frames();
        clock.advance_to(clock.at(boundary(1) - Duration::from_nanos(1)));
        crtc(&mut *vga.borrow_mut(), 12, 0x12);
        clock.advance_to(clock.at(boundary(1)));
        crtc(&mut *vga.borrow_mut(), 13, 0x34);
        assert!(source.is_empty());
        common::service_video(video.as_mut());
        let first = source.try_recv().unwrap();
        assert_eq!(first.vga().unwrap().display_start, 0x1200);
        assert_eq!(&first.vga().unwrap().crtc.values[12..14], &[0x12, 0]);
        assert_eq!(
            vga.borrow_mut().read_port(context.reborrow(), 0x3d5, 1),
            0x34
        );
        assert_eq!(vga.borrow().snapshot().vga().unwrap().display_start, 0x1200);
        clock.advance_to(clock.at(boundary(2)));
        vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
        common::service_video(video.as_mut());
        let next = source.try_recv().unwrap();
        assert_eq!(next.vga().unwrap().display_start, 0x1234);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn late_service_latches_preceding_programming_before_a_new_write() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = common::start_video(Vga::with_clock(clock.clock()));
        let source = vga.borrow().frames();
        start(&mut *vga.borrow_mut(), 0x100);
        clock.advance_to(clock.at(boundary(100)));
        start(&mut *vga.borrow_mut(), 0x200);
        assert!(source.is_empty());
        common::service_video(video.as_mut());
        let latest = source.try_recv().unwrap();

        assert_eq!(latest.vga().unwrap().display_start, 0x100);
        assert_eq!(vga.borrow().snapshot().vga().unwrap().display_start, 0x100);
        clock.advance_to(clock.at(boundary(101)));
        vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
        common::service_video(video.as_mut());
        let next = source.try_recv().unwrap();
        assert_eq!(next.vga().unwrap().display_start, 0x200);
        assert_eq!(latest.vga().unwrap().display_start, 0x100);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_copy_the_effective_latch_without_servicing_time() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = common::start_video(Vga::with_clock(clock.clock()));
        let source = vga.borrow().frames();
        start(&mut *vga.borrow_mut(), 0x1234);
        clock.advance_to(clock.at(boundary(1)));
        let snapshot = vga.borrow().snapshot();
        assert_eq!(&snapshot.vga().unwrap().crtc.values[12..14], &[0x12, 0x34]);
        assert_eq!(snapshot.vga().unwrap().display_start, 0);
        assert!(source.is_empty());
        vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
        common::service_video(video.as_mut());
        let next = source.try_recv().unwrap();
        assert_eq!(next.vga().unwrap().display_start, 0x1234);
        assert_eq!(snapshot.vga().unwrap().display_start, 0);
    })
}

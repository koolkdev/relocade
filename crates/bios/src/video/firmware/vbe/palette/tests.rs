use super::*;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Clock, Device, MachineConfig, Memory};
use relocade_vga::{DispiAdapter, DispiMode, DispiResolution};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    future::Future,
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

const BUFFER: FarPointer = FarPointer {
    segment: 0x5000,
    offset: 0x100,
};

struct Palette {
    card: Rc<RefCell<DispiAdapter>>,
    io: IoBus,
    memory: Memory,
}

impl Palette {
    fn new(clock: Clock) -> Self {
        let map =
            PhysicalMemoryMap::new([(0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap();
        let memory = Memory::new(MachineConfig::new(0xa0000, map).unwrap()).unwrap();
        memory
            .view()
            .write_wrapping(BUFFER, &[1, 2, 3, 0xee])
            .unwrap();
        let card = Rc::new(RefCell::new(DispiAdapter::with_clock(clock)));
        let io = IoBus::new(card.clone(), |io| DispiAdapter::configure_io(io, &card));
        io.with_context(|context| {
            assert!(card.borrow_mut().enter_packed_mode(
                context,
                DispiMode::new(DispiResolution::R640x480, PixelFormat::Indexed8),
                true,
            ));
        });
        io.write_port(0x3c8, 1, 17);
        for component in [9, 8, 7] {
            io.write_port(0x3c9, 1, component);
        }
        Self { card, io, memory }
    }
}

fn boundary(frame: u64) -> Duration {
    Duration::from_nanos(((490 + (frame - 1) * 525) * 800 * 1_000_000_000).div_ceil(25_175_000))
}

#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn retrace_update_owns_the_input_and_preserves_completed_colors() {
    let clock = ManualClock::default();
    let Palette { card, io, memory } = Palette::new(clock.clock());
    let mut view = memory.view();
    let mut cx = Context::from_waker(Waker::noop());
    let frames = card.borrow().frames();
    let mut video = pin!(DispiAdapter::run(card.clone()));
    assert!(video.as_mut().poll(&mut cx).is_pending());
    let mut request = pin!(set_at_retrace(&mut view, &io, card.as_ref(), 1, 17, BUFFER));
    let dac = card.borrow().dac_state();
    assert!(request.as_mut().poll(&mut cx).is_pending());
    assert_eq!(card.borrow().dac_state(), dac);
    assert_eq!(clock.next_deadline(), Some(clock.at(boundary(1))));

    // An interrupt handler can overwrite the caller's buffer while it is parked.
    memory.view().write_wrapping(BUFFER, &[4, 5, 6, 0]).unwrap();
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
    assert_eq!(card.borrow().registers().dac.colors[17], [3, 2, 1]);
    assert!(video.as_mut().poll(&mut cx).is_pending());
    let completed = frames.try_recv().unwrap();
    assert_eq!(completed.dac.colors[17], [9, 8, 7]);
    clock.advance_to(clock.at(boundary(2)));
    io.read_port(0x3da, 1);
    assert!(video.as_mut().poll(&mut cx).is_pending());
    assert_eq!(frames.try_recv().unwrap().dac.colors[17], [3, 2, 1]);
    assert_eq!(completed.dac.colors[17], [9, 8, 7]);
}

#[test]
fn mode_changes_and_disabled_decoding_cancel_without_programming_the_palette() {
    for mode in [
        Some(DispiMode::new(
            DispiResolution::R800x600,
            PixelFormat::Indexed8,
        )),
        Some(DispiMode::new(
            DispiResolution::R640x480,
            PixelFormat::Rgb565,
        )),
        None,
    ] {
        let clock = ManualClock::default();
        let Palette { card, io, memory } = Palette::new(clock.clock());
        let mut view = memory.view();
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut request = pin!(set_at_retrace(&mut view, &io, card.as_ref(), 1, 17, BUFFER));
        assert!(request.as_mut().poll(&mut cx).is_pending());
        if let Some(mode) = mode {
            io.with_context(|context| {
                assert!(card.borrow_mut().enter_packed_mode(context, mode, true));
            });
        } else {
            io.write_port(0x46e8, 1, 0);
        }
        assert!(wakes.0.load(Ordering::SeqCst) > 0);
        let dac = card.borrow().dac_state();
        assert_eq!(
            request.as_mut().poll(&mut cx),
            Poll::Ready(Err(Error::Failed))
        );
        assert_eq!(card.borrow().dac_state(), dac);
        assert_eq!(clock.next_deadline(), None);
    }
}

#[test]
fn pitch_origin_and_status_routing_changes_rearm_a_valid_palette_request() {
    let clock = ManualClock::default();
    let Palette { card, io, memory } = Palette::new(clock.clock());
    let mut view = memory.view();
    let mut cx = Context::from_waker(Waker::noop());
    let mut request = pin!(set_at_retrace(&mut view, &io, card.as_ref(), 1, 17, BUFFER));
    assert!(request.as_mut().poll(&mut cx).is_pending());
    io.with_context(|context| {
        assert!(card.borrow_mut().set_display_pitch(context, 1024).is_some());
    });
    io.with_context(|context| {
        assert!(card.borrow_mut().set_display_origin(context, [32, 0]));
    });
    io.write_port(0x3c2, 1, 2); // Move status to the monochrome port.
    assert!(request.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(clock.at(boundary(1) + Duration::from_millis(1)));
    assert_eq!(io.read_port(0x3ba, 1) & 8, 0); // The pulse has ended before resumption.
    assert_eq!(request.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
    assert_eq!(card.borrow().registers().dac.colors[17], [3, 2, 1]);
    assert_eq!(card.borrow().viewport().unwrap().pitch(), 1024);
    assert_eq!(card.borrow().viewport().unwrap().origin(), [32, 0]);
}

#[test]
fn empty_invalid_and_already_in_retrace_requests_do_not_wait() {
    let clock = ManualClock::default();
    let Palette { card, io, memory } = Palette::new(clock.clock());
    let mut view = memory.view();
    let mut cx = Context::from_waker(Waker::noop());
    let dac = card.borrow().dac_state();
    for (count, first, result) in [(0, 17, Ok(())), (2, 255, Err(Error::Failed))] {
        assert_eq!(
            pin!(set_at_retrace(
                &mut view,
                &io,
                card.as_ref(),
                count,
                first,
                BUFFER
            ))
            .poll(&mut cx),
            Poll::Ready(result)
        );
        assert_eq!(card.borrow().dac_state(), dac);
        assert_eq!(clock.next_deadline(), None);
    }
    clock.advance_to(clock.at(boundary(1)));
    assert_eq!(
        pin!(set_at_retrace(&mut view, &io, card.as_ref(), 1, 17, BUFFER)).poll(&mut cx),
        Poll::Ready(Ok(()))
    );
    assert_eq!(card.borrow().registers().dac.colors[17], [3, 2, 1]);
    assert_eq!(clock.next_deadline(), None);
}

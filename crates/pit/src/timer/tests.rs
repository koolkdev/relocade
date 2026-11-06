use super::*;
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

#[derive(Clone, Default)]
struct ManualClock(Rc<Time>);

#[derive(Default)]
struct Time {
    now: Cell<Duration>,
    waiter: RefCell<Option<Waker>>,
}

impl ManualClock {
    fn advance_to(&self, nanoseconds: u64) {
        let now = Duration::from_nanos(nanoseconds);
        assert!(now >= self.0.now.get());
        self.0.now.set(now);
        if let Some(waker) = self.0.waiter.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.now.get()
    }

    async fn wait_until(&self, deadline: Duration) {
        poll_fn(|cx| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                *self.0.waiter.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn default_rate_wakes_the_cpu_and_coalesces_late_edges_without_drift() {
    let clock = ManualClock::default();
    clock.advance_to(1_000_000_000);
    let mut pic = relocade_pic::Pic::new(0x08);
    let timer = Rc::new(RefCell::new(Pit::with_clock(Rc::new(clock.clone()), &pic)));
    pic.unmask_irq(relocade_pic::TIMER_IRQ);
    let mut activity = pin!(Pit::run(timer.clone()));
    let mut device_cx = Context::from_waker(Waker::noop());
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cpu_cx = Context::from_waker(&waker);

    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance_to(1_054_925_401);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);

    clock.advance_to(1_054_925_402);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(pic.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    pic.write_port(0x20, 1, 0x20);
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());

    // A replacement CPU waiter must receive subsequent edges.
    let replaced = wakes;
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cpu_cx = Context::from_waker(&waker);
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());

    // A delayed host poll creates one pending request and keeps the original phase.
    clock.advance_to(1_549_254_012 + 1_234);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(replaced.0.load(Ordering::Relaxed), 1);
    assert_eq!(pic.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    pic.write_port(0x20, 1, 0x20);
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());
    // The eleventh boundary is ceil(11 * 65536 * 1e9 / 1193182),
    // not eleven separately rounded periods.
    clock.advance_to(1_604_179_412);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());
    clock.advance_to(1_604_179_413);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
    assert_eq!(pic.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    pic.write_port(0x20, 1, 0x20);

    // Edges also coalesce when the activity runs but the CPU does not acknowledge.
    clock.advance_to(1_659_104_814);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    clock.advance_to(1_714_030_216);
    assert!(activity.as_mut().poll(&mut device_cx).is_pending());
    assert_eq!(pic.poll_interrupt(&mut cpu_cx), Poll::Ready(0x08));
    pic.write_port(0x20, 1, 0x20);
    assert!(pic.poll_interrupt(&mut cpu_cx).is_pending());
}

fn program(pit: &mut impl Device, control: u8, divisor: u16) {
    pit.write_port(CONTROL_PORT, 1, control.into());
    for byte in divisor.to_le_bytes() {
        pit.write_port(CHANNEL_0_PORT, 1, byte.into());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn programming_rearms_the_wait_and_preserves_pending_irq0() {
    let clock = ManualClock::default();
    let mut pic = relocade_pic::Pic::new(8);
    let pit = Rc::new(RefCell::new(Pit::with_clock(Rc::new(clock.clone()), &pic)));
    pic.unmask_irq(relocade_pic::TIMER_IRQ);
    let mut activity = pin!(Pit::run(pit.clone()));
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    program(&mut *pit.borrow_mut(), 0x34, 4);
    assert_ne!(wakes.0.load(Ordering::Relaxed), 0);
    assert!(activity.as_mut().poll(&mut cx).is_pending()); // Cancel the BIOS-rate wait.
    assert!(activity.as_mut().poll(&mut cx).is_pending()); // Arm the new load.

    clock.advance_to(3_353); // Four clocks: load, then mode-2 OUT low.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(pic.poll_interrupt(&mut cx).is_pending());
    clock.advance_to(4_191); // Fifth clock: first rising edge.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(pic.read_port(relocade_pic::COMMAND_PORT, 1) & 1, 1);
    // Stop the timer before acknowledging; programming must not clear PIC IRR.
    program(&mut *pit.borrow_mut(), 0x30, 4);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(8));
    pic.write_port(0x20, 1, 0x20);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    clock.advance_to(1_000_000_000);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(pic.poll_interrupt(&mut cx).is_pending()); // No stale deadline IRQ.

    program(&mut *pit.borrow_mut(), 0x36, 6);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(8)); // Control forces OUT high.
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn readback_latches_both_channels_without_changing_speaker_playback() {
    let clock = ManualClock::default();
    let mut pit = Pit::with_clock(Rc::new(clock.clone()), &relocade_pic::Pic::new(8));
    program(&mut pit, 0x34, 4);
    pit.write_port(CONTROL_PORT, 1, 0xb6);
    pit.write_port(CHANNEL_2_PORT, 1, 6);
    pit.write_port(CHANNEL_2_PORT, 1, 0);
    pit.set_channel2_gate(true);
    pit.take_channel2_update();
    clock.advance_to(2_515); // Three clocks: load plus two decrements.
    pit.write_port(CONTROL_PORT, 1, 0xca); // Count and status for channels 0 and 2.
    clock.advance_to(5_029);
    assert_eq!(pit.read_port(CHANNEL_0_PORT, 1), 0xb4);
    assert_eq!(pit.read_port(CHANNEL_0_PORT, 1), 2);
    assert_eq!(pit.read_port(CHANNEL_0_PORT, 1), 0);
    assert_eq!(pit.read_port(CHANNEL_2_PORT, 1), 0xb6);
    assert_eq!(pit.read_port(CHANNEL_2_PORT, 1), 2);
    assert_eq!(pit.read_port(CHANNEL_2_PORT, 1), 0);
    program(&mut pit, 0x36, 8);
    assert!(pit.take_channel2_update().is_none());
    assert_eq!(pit.read_port(0x41, 1), u32::MAX);
    assert_eq!(pit.read_port(CHANNEL_0_PORT, 2), u32::MAX);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn irq0_tracks_output_levels_for_a_level_triggered_pic() {
    let clock = ManualClock::default();
    let mut pic = relocade_pic::Pic::new(8);
    let pit = Rc::new(RefCell::new(Pit::with_clock(Rc::new(clock.clone()), &pic)));
    program(&mut *pit.borrow_mut(), 0x36, 4);
    // Guest selects level-triggered operation.
    pic.write_port(0x20, 1, 0x1b);
    pic.write_port(0x21, 1, 8);
    pic.write_port(0x21, 1, 1);
    pic.write_port(0x21, 1, 0xfe);
    let mut activity = pin!(Pit::run(pit.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(8)); // Initial high level.
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(8)); // Still high after EOI.
    clock.advance_to(2_515); // Load plus the high half-period.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    pic.write_port(0x20, 1, 0x20);
    assert!(pic.poll_interrupt(&mut cx).is_pending());
    clock.advance_to(4_191); // Low half-period ends.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(8));
    pic.write_port(0x20, 1, 0x20);
    clock.advance_to(9_220); // Several transitions later, OUT is low again.
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(pic.poll_interrupt(&mut cx).is_pending());
}

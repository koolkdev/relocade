use super::*;
use crate::Registers;
use relocade_clock_test_support::ManualClock;
use std::time::Duration;

impl Raster {
    fn update_to(&mut self, now: Timestamp) -> Option<Timestamp> {
        self.status
            .observe(now)
            .then(|| self.update_observed())
            .flatten()
    }
}

fn dots(count: u64) -> Duration {
    Duration::from_nanos((count * 1_000_000_000).div_ceil(25_175_000))
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn observations_reuse_status_and_phase_until_the_next_edge() {
    let controller = ManualClock::new();
    let clock = controller.clock();
    let origin = clock.now();
    let mut raster = Raster::new(clock.clone(), Timing::vga(&Registers::default()));
    let retrace = controller.at(dots(412 * 800));
    assert_eq!(raster.next_retrace_at(), Some(retrace));
    assert_eq!(
        raster.status.next_update.get(),
        Some(controller.at(dots(640)))
    );
    for count in [1, 8, 127, 639] {
        assert_eq!(raster.update_to(controller.at(dots(count))), None);
        assert_eq!(raster.status.input_status_1(), 0);
        // Avoiding phase materialization is the optimization being protected.
        assert_eq!(raster.updated, origin);
        assert_eq!(raster.dot, 0);
        assert_eq!(raster.next_retrace_at(), Some(retrace));
    }
    raster.update_to(controller.at(dots(640) - Duration::from_nanos(1)));
    assert_eq!(raster.updated, origin);
    raster.update_to(controller.at(dots(640)));
    assert_eq!(raster.status.input_status_1(), 1);
    assert_eq!(raster.updated, controller.at(dots(640)));
    assert_eq!(
        raster.status.next_update.get(),
        Some(controller.at(dots(800)))
    );
    assert_eq!(raster.next_retrace_at(), Some(retrace));
    let late = controller.at(dots((412 + 99 * 449) * 800));
    assert_eq!(raster.update_to(late), Some(late));
    assert_eq!(raster.retrace_count, 100);
    assert_eq!(raster.status.input_status_1(), 9);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn skewed_display_edges_skip_horizontal_work_outside_vertical_display() {
    let mut timing = Timing::vga(&Registers::default());
    timing.horizontal_total = 40;
    timing.horizontal_display_skew = 8;
    timing.horizontal_display = 16;
    timing.vertical_total = 3;
    timing.vertical_display = 2;
    // The ordinary retrace start is outside this short frame.
    let controller = ManualClock::new();
    let clock = controller.clock();
    let mut raster = Raster::new(clock.clone(), timing);
    for (dot, status, next) in [
        (0, 1, 8),
        (8, 0, 24),
        (24, 1, 48),
        (48, 0, 64),
        (64, 1, 80),
        (80, 1, 120),
        (120, 1, 128),
    ] {
        raster.update_to(controller.at(dots(dot)));
        assert_eq!(raster.status.input_status_1(), status, "dot {dot}");
        assert_eq!(
            raster.status.next_update.get(),
            Some(controller.at(dots(next))),
            "dot {dot}"
        );
        let updated = raster.updated;
        raster.update_to(controller.at(dots(next) - Duration::from_nanos(1)));
        assert_eq!(raster.status.input_status_1(), status);
        assert_eq!(raster.updated, updated);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clock_reprogramming_materializes_lazy_phase_and_excludes_stopped_time() {
    let mut registers = Registers::default();
    let controller = ManualClock::new();
    let clock = controller.clock();
    let origin = clock.now();
    let mut raster = Raster::new(clock.clone(), Timing::vga(&registers));
    let status = raster.status.clone();
    let stopped_at = controller.at(dots(100));
    raster.update_to(stopped_at);
    assert_eq!(raster.updated, origin);
    registers.miscellaneous = 0x4b; // Disconnected oscillator; preserve counters.
    raster.set_timing(Timing::vga(&registers));
    assert_eq!(raster.updated, stopped_at);
    assert_eq!(raster.dot, 100);
    assert_eq!(status.next_update.get(), None);
    let resumed_at = controller.at(Duration::from_secs(1));
    raster.update_to(resumed_at);
    assert_eq!(raster.updated, stopped_at);
    registers.miscellaneous = 0x43;
    raster.set_timing(Timing::vga(&registers));
    assert_eq!(raster.updated, resumed_at);
    assert_eq!(raster.dot, 100);
    // Original fractional progress survives the pause.
    let edge = clock.add(resumed_at, dots(640) - dots(100));
    assert_eq!(status.next_update.get(), Some(edge));
    raster.update_to(clock.sub(edge, Duration::from_nanos(1)));
    assert_eq!(status.input_status_1(), 0);
    raster.update_to(edge);
    assert_eq!(status.input_status_1(), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fractional_phase_survives_programming_until_an_explicit_restart() {
    use relocade_machine::GuestTimeDriver;
    use std::{future::Future, num::NonZeroU64, pin::pin, task::Context, task::Waker};

    let driver = GuestTimeDriver::new(NonZeroU64::new(3).unwrap());
    let clock = driver.clock();
    let mut initial = pin!(clock.sleep_until(Timestamp::from_ticks(1)));
    assert!(initial
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert!(driver.advance_to_next_deadline());
    let mut timing = Timing::vga(&Registers::default());
    let mut raster = Raster::new(clock.clone(), timing);
    let edge = clock.add(clock.now(), dots(640));
    assert_eq!(raster.status.next_update.get(), Some(edge));

    raster.update_to(Timestamp::from_ticks(edge.ticks() - 1));
    timing.vertical_total += 1;
    raster.set_timing(timing);
    assert_eq!(raster.status.input_status_1(), 0);
    assert_eq!(raster.updated, clock.sub(edge, Duration::from_nanos(1)));
    assert_eq!(raster.status.next_update.get(), Some(edge));
    raster.update_to(edge);
    assert_eq!(raster.status.input_status_1(), 1);

    let restart = Timestamp::from_ticks(edge.ticks() + 1);
    raster.update_to(restart);
    raster.restart(timing);
    assert_eq!(raster.updated, restart);
    assert_eq!(
        raster.status.next_update.get(),
        Some(clock.add(restart, dots(640)))
    );

    timing.reset_active = true;
    raster.set_timing(timing);
    let released = Timestamp::from_ticks(restart.ticks() + 1);
    raster.update_to(released);
    timing.reset_active = false;
    raster.set_timing(timing);
    assert_eq!(raster.updated, released);
    assert_eq!(
        raster.status.next_update.get(),
        Some(clock.add(released, dots(640)))
    );
}

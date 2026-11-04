use super::Counter;

fn programmed(control: u8, divisor: u16) -> Counter {
    let mut counter = Counter::default();
    counter.set_gate(true);
    counter.control(control);
    for byte in divisor.to_le_bytes() {
        counter.write(byte);
    }
    counter
}

fn count(counter: &mut Counter) -> u16 {
    counter.latch(true, false);
    u16::from_le_bytes([counter.read(), counter.read()])
}

fn status(counter: &mut Counter) -> u8 {
    counter.latch(false, true);
    counter.read()
}

#[test]
fn documented_count_and_output_sequences() {
    // Intel 8254 mode-2 and mode-3 timing diagrams, starting at initial load.
    for (control, divisor, sequence) in [
        (0x34, 3, &[(3, true), (2, true), (1, false), (3, true)][..]),
        (
            0x36,
            4,
            &[(4, true), (2, true), (4, false), (2, false), (4, true)][..],
        ),
        (
            0x36,
            5,
            &[
                (4, true),
                (2, true),
                (0, true),
                (4, false),
                (2, false),
                (4, true),
            ][..],
        ),
    ] {
        let mut counter = programmed(control, divisor);
        assert_eq!(count(&mut counter), 0);
        for &(expected_count, expected_output) in sequence {
            counter.advance(1);
            assert_eq!(
                (count(&mut counter), counter.output()),
                (expected_count, expected_output)
            );
        }
    }
}

#[test]
fn reload_waits_for_period_or_half_period() {
    for (control, until_reload, output_after_reload) in [(0x34, 3, true), (0x36, 1, false)] {
        let mut counter = programmed(control, 4);
        counter.advance(2); // Initial load and one decrement.
        counter.write(6);
        assert_eq!(status(&mut counter) & 0x40, 0); // First byte is incomplete.
        counter.write(0);
        assert_eq!(status(&mut counter) & 0x40, 0x40);
        counter.advance(until_reload - 1);
        assert_eq!(status(&mut counter) & 0x40, 0x40);
        counter.advance(1);
        assert_eq!(status(&mut counter) & 0x40, 0);
        assert_eq!(count(&mut counter), 6);
        assert_eq!(counter.output(), output_after_reload);
    }

    let mut counter = programmed(0x36, 4);
    counter.advance(3); // Low half, with two clocks remaining.
    counter.write(5);
    counter.write(0);
    assert_eq!(counter.advance(17), 9); // Two low clocks, then three odd periods.
    assert_eq!(count(&mut counter), 4);
    assert!(counter.output());
    assert_eq!(status(&mut counter), 0xb6);
}

#[test]
fn gate_stops_and_retriggers_without_retriggering_repeated_high() {
    let mut counter = programmed(0x36, 4);
    counter.advance(3);
    assert!(!counter.output());
    counter.set_gate(false);
    assert!(counter.output());
    counter.advance(100);
    assert_eq!(count(&mut counter), 4);
    counter.write(8);
    counter.write(0);
    counter.set_gate(true);
    assert!(counter.output());
    counter.advance(1);
    assert_eq!(count(&mut counter), 8);
    counter.advance(1);
    counter.set_gate(true);
    counter.advance(1);
    assert_eq!(count(&mut counter), 4);
}

#[test]
fn count_and_status_latches_are_independent_and_preserve_first_value() {
    let mut counter = programmed(0x36, 0x1234);
    assert_eq!(status(&mut counter), 0xf6);
    counter.advance(1);
    counter.latch(true, true);
    counter.advance(5);
    counter.latch(true, true);
    assert_eq!(counter.read(), 0xb6);
    assert_eq!(counter.read(), 0x34);
    counter.write(8); // Write byte position is independent of reading.
    counter.latch(false, true);
    assert_eq!(counter.read(), 0xb6); // Status does not consume the count MSB.
    counter.write(0);
    assert_eq!(counter.read(), 0x12);
    assert_eq!(status(&mut counter), 0xf6);
    assert_eq!(count(&mut counter), 0x122a);
}

#[test]
fn count_latch_control_does_not_reprogram_and_alias_status_is_preserved() {
    for control in [0x3c, 0x3e] {
        let mut counter = programmed(control, 8);
        counter.advance(1);
        counter.control(0);
        counter.advance(2);
        assert_eq!(u16::from_le_bytes([counter.read(), counter.read()]), 8);
        assert_eq!(status(&mut counter), 0x80 | control);
        assert_eq!(count(&mut counter), if control == 0x3c { 6 } else { 4 });
    }
}

#[test]
fn single_byte_access_zeroes_the_other_byte_and_zero_means_65536() {
    let mut counter = Counter::default();
    counter.set_gate(true);
    counter.control(0x14);
    counter.write(7);
    counter.advance(1);
    assert_eq!(counter.read(), 7);
    counter.control(0x24);
    counter.write(7);
    counter.advance(1);
    assert_eq!(counter.read(), 7);
    counter.advance(1);
    assert_eq!(counter.read(), 6);
    let mut counter = programmed(0x34, 0);
    counter.advance(1);
    assert_eq!(count(&mut counter), 0);
    counter.advance(1);
    assert_eq!(count(&mut counter), 65535);
    counter.advance(65534);
    assert!(!counter.output());
    counter.advance(1);
    assert!(counter.output());
}

#[test]
fn unsupported_programming_stops_the_previous_oscillator() {
    for control in [0x30, 0x32, 0x38, 0x3a, 0x35, 0x37] {
        let mut counter = programmed(0x36, 4);
        counter.advance(1);
        counter.control(control);
        counter.write(4);
        counter.write(0);
        assert_eq!(counter.advance(100), 0);
        assert!(!counter.output());
    }
    let mut counter = programmed(0x36, 1);
    assert_eq!(counter.advance(100), 100);
    assert_eq!(count(&mut counter), 0);
}

#[test]
fn high_time_integrates_large_jumps_and_partial_periods() {
    let mut counter = programmed(0x36, 5);
    assert_eq!(counter.advance(1), 1);
    assert_eq!(counter.advance(2), 2);
    assert_eq!(counter.advance(5_000_000_003), 3_000_000_001);
    assert_eq!(count(&mut counter), 4);
    assert!(counter.output());
    let mut counter = programmed(0x34, 3);
    counter.advance(1);
    assert_eq!(counter.advance(3_000_000_000), 2_000_000_000);
}

#[test]
fn edge_deadlines_follow_loads_and_mode_specific_reload_boundaries() {
    for (control, first_fall, reloaded_rise) in [(0x34, 4, 5), (0x36, 3, 6)] {
        let mut counter = programmed(control, 4);
        assert_eq!(counter.next_transition(), Some(1));
        assert_eq!(counter.next_rising_edge(), Some(5));
        counter.advance(2); // Loaded, one decrement elapsed.
        counter.write(6);
        counter.write(0);
        assert_eq!(counter.next_transition(), Some(first_fall - 2));
        assert_eq!(counter.next_rising_edge(), Some(reloaded_rise - 2));
        counter.advance(reloaded_rise - 2);
        assert!(counter.output());
        assert_eq!(counter.next_rising_edge(), Some(6));
        counter.set_gate(false);
        assert_eq!(counter.next_transition(), None);
        assert_eq!(counter.next_rising_edge(), None);
        counter.set_gate(true);
        assert_eq!(counter.next_rising_edge(), Some(7));
    }
    let mut counter = programmed(0x36, 5);
    counter.advance(4); // Start of the odd divisor's two-clock low phase.
    counter.write(6);
    counter.write(0);
    assert_eq!(counter.next_rising_edge(), Some(2));
    counter.control(0x30);
    assert_eq!(counter.next_transition(), None);
    assert_eq!(counter.next_rising_edge(), None);
}

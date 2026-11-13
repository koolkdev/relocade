use super::*;

fn deadline(clock: &ManualClock, device: &ConnectedEt4000) -> Duration {
    let mut wait = pin!(device.card.borrow_mut().wait_for_retrace().unwrap());
    assert!(wait
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    clock.elapsed(clock.next_deadline().unwrap())
}

fn dot_time(dots: u64, hz: u64) -> Duration {
    Duration::from_nanos((dots * 1_000_000_000).div_ceil(hz))
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extended_clock_selection_and_division_use_the_board_oscillators() {
    for (compatibility, miscellaneous, auxiliary, frequency, divisor) in [
        (0x08, 0x4f, 0xbc, 40_000_000, 1),
        (0x0a, 0x4f, 0xbc, 44_900_000, 1),
        (0x0a, 0x43, 0xbc, 25_175_000, 1),
        (0x0a, 0x4f, 0xfc, 44_900_000, 2),
        (0x0a, 0x4f, 0xfd, 44_900_000, 4),
    ] {
        let clock = ManualClock::default();
        let device = ConnectedEt4000::new(&clock);
        let io = &device.io;
        io.write_port(0x3bf, 1, 3);
        io.write_port(0x3d8, 1, 0xa0);
        io.write_port(0x3d4, 2, (compatibility << 8) | 0x34);
        io.write_port(0x3c4, 2, (auxiliary << 8) | 7);
        io.write_port(0x3c2, 1, miscellaneous);
        assert_eq!(
            deadline(&clock, &device),
            dot_time(412 * 800 * divisor, frequency)
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overflow_bits_extend_geometry_and_the_retrace_counter() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    scanout::indexed_640(&device.io);
    device.io.write_port(0x3d4, 2, 0x0011);
    device.io.write_port(0x3d4, 2, 0x0e35); // Bit 10 of total, display and retrace.
    assert_eq!(scanout::layout(&device).size(), [640, 1504]);
    assert_eq!(
        deadline(&clock, &device),
        dot_time((490 + 1024) * 800, 25_175_000)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn data_book_800_by_600_and_interlaced_1024_by_768_have_programmed_timing() {
    for interlaced in [false, true] {
        let clock = ManualClock::default();
        let device = ConnectedEt4000::new(&clock);
        let io = &device.io;
        scanout::indexed_640(io);
        io.write_port(0x3d4, 2, 0x0011);
        // Tables 3.2.4-1/3: mode 30h and 38h interlaced. Clock seven is 44.9 MHz.
        let registers = if interlaced {
            [
                0x99, 0x7f, 0x7f, 0x1d, 0x83, 0x17, 0x2f, 0xf5, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x80,
                0xff, 0x80, 0x60, 0xff, 0x30, 0xab, 0xff,
            ]
        } else {
            [
                0x7f, 0x63, 0x64, 0x02, 0x64, 0x17, 0x77, 0xf0, 0, 0x60, 0, 0, 0, 0, 0, 0, 0x60,
                0x82, 0x57, 0x64, 0x60, 0x5b, 0x75, 0xab, 0xff,
            ]
        };
        for (index, value) in registers.into_iter().enumerate() {
            io.write_port(0x3d4, 2, (value << 8) | index as u32);
        }
        io.write_port(0x3d4, 2, if interlaced { 0x8035 } else { 0x0035 });
        io.write_port(0x3d4, 2, if interlaced { 0x0a34 } else { 0x0834 });
        io.write_port(0x3c2, 1, if interlaced { 0x2f } else { 0xef });
        let (size, pitch, total, retrace, frequency) = if interlaced {
            (
                [1024, 768],
                1024,
                1264 * 817 / 2,
                1264 * 776 / 2,
                44_900_000,
            )
        } else {
            ([800, 600], 800, 1056 * 633, 1056 * 608, 40_000_000)
        };
        assert_eq!(scanout::layout(&device).size(), size);
        assert_eq!(scanout::layout(&device).pitch(), pitch);
        assert_eq!(deadline(&clock, &device), dot_time(retrace, frequency));
        clock.advance_to(clock.at(dot_time(retrace, frequency) - Duration::from_nanos(1)));
        assert_eq!(io.read_port(0x3da, 1) & 8, 0);
        clock.advance_to(clock.at(dot_time(retrace, frequency)));
        assert_eq!(io.read_port(0x3da, 1) & 8, 8);
        // After this pulse ends, the next wait follows the next field/frame.
        clock.advance_to(clock.at(dot_time(retrace + 20 * 1264, frequency)));
        io.read_port(0x3da, 1);
        assert_eq!(
            deadline(&clock, &device),
            dot_time(retrace + total, frequency)
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn all_eighteen_start_bits_latch_together_and_overdue_capture_precedes_mutation() {
    let clock = ManualClock::default();
    let device = ConnectedEt4000::new(&clock);
    let io = &device.io;
    scanout::indexed_640(io);
    io.write_port(0x3d4, 2, 0x0133);
    io.write_port(0x3d4, 2, 0x230c);
    io.write_port(0x3d4, 2, 0x450d);
    assert_eq!(scanout::layout(&device).display_start(), 0);
    let edge = deadline(&clock, &device);
    clock.advance_to(clock.at(edge));
    // This write must first capture the address that was pending at the edge.
    io.write_port(0x3d4, 2, 0x0233);
    assert_eq!(scanout::layout(&device).display_start(), 0x48d14);
    clock.advance_to(clock.at(edge + dot_time(525 * 800, 25_175_000)));
    io.read_port(0x3da, 1);
    assert_eq!(scanout::layout(&device).display_start(), 0x88d14);
}

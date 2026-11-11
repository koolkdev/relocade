use crate::Vga;
use relocade_clock_test_support::ManualClock;
use relocade_machine::IoBus;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_register_writes_skip_raster_work_and_vga_effects_resume_at_mode_boundaries() {
    let clock = ManualClock::new();
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    let io = IoBus::new(vga.clone(), |io| Vga::configure_io(io, &vga));
    let legacy = vga.borrow().dispi_state();
    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 0x81)] {
        io.write_port(0x1ce, 2, index);
        io.write_port(0x1cf, 2, value);
    }
    let packed = vga.borrow().dispi_state();

    // Select each indexed bank once. Mode changes and DISPI-only restores must
    // update its data writer without relying on another index write.
    for (index_port, data_port, index, value) in [
        (0x3d4, 0x3d5, 0x0d, 0x17),
        (0x3c4, 0x3c5, 2, 0x0f),
        (0x3ce, 0x3cf, 8, 0x5a),
        (0x3c0, 0x3c0, 0x12, 7),
    ] {
        io.write_port(index_port, 1, index);
        for transition in 0..4 {
            match transition {
                0 => {
                    // The preceding bank ended in the saved VGA state.
                    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 0x81)] {
                        io.write_port(0x1ce, 2, index);
                        io.write_port(0x1cf, 2, value);
                    }
                }
                1 => {
                    io.write_port(0x1ce, 2, 4);
                    io.write_port(0x1cf, 2, 0);
                }
                2 => io.with_context(|context| {
                    vga.borrow_mut()
                        .restore_state(context, None, None, Some(packed.clone()))
                }),
                3 => io.with_context(|context| {
                    vga.borrow_mut()
                        .restore_state(context, None, None, Some(legacy.clone()))
                }),
                _ => unreachable!(),
            }
            let packed = transition % 2 == 0;
            io.read_port(0x3da, 1); // Service old work and reset Attribute phase.
            let before = vga.borrow().raster.retrace_count;
            clock.advance_by(Duration::from_millis(20));
            if index_port == data_port {
                // Attribute has its own address/data flip-flop protocol.
                io.write_port(index_port, 1, index);
            }
            io.write_port(data_port, 1, value);
            let after = vga.borrow().raster.retrace_count;
            assert_eq!(
                after == before,
                packed,
                "port {data_port:x}, transition {transition}"
            );
            let read_port = if data_port == 0x3c0 { 0x3c1 } else { data_port };
            assert_eq!(io.read_port(read_port, 1), value);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn indexed_transfers_apply_the_new_effect_before_the_data_byte() {
    let clock = ManualClock::new();
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    let io = IoBus::new(vga.clone(), |io| Vga::configure_io(io, &vga));
    io.write_port(0x3c4, 1, 7); // An unimplemented index has no display work.
    let before = vga.borrow().raster.retrace_count;
    clock.advance_by(Duration::from_millis(20));
    // The raw index 0A masks to Map Mask (02), whose write preserves display inputs.
    io.write_port(0x3c4, 2, 0x0f0a);
    assert_eq!(io.read_port(0x3c4, 2), 0x0f02);
    assert!(vga.borrow().raster.retrace_count > before);
    let before = vga.borrow().raster.retrace_count;
    clock.advance_by(Duration::from_millis(20));
    io.write_port(0x3c5, 1, 5); // Repeated data writes retain that effect.
    assert!(vga.borrow().raster.retrace_count > before);

    let before = vga.borrow().raster.retrace_count;
    clock.advance_by(Duration::from_millis(20));
    io.write_port(0x3c4, 2, 0x0007); // Selecting an invalid index removes display work.
    assert_eq!(vga.borrow().raster.retrace_count, before);
    assert_eq!(io.read_port(0x3c5, 1), 0xff);

    // Selecting Reset must also reconcile timing after its data byte. Release it
    // through a subsequent data-only write, retaining the same timing handler.
    io.write_port(0x3c4, 2, 0x0100);
    assert_eq!(vga.borrow().raster.next_retrace_at(), None);
    io.write_port(0x3c5, 1, 3);
    assert!(vga.borrow().raster.next_retrace_at().is_some());
}

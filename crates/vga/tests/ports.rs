//! Register and status operations use the PC's connection and configuration paths.
mod common;
#[path = "ports/guest.rs"]
mod guest;

use relocade_clock_test_support::ManualClock;
use relocade_machine::{Device, IoBus, PortPolicy};
use relocade_vga::Vga;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn connect(clock: &ManualClock) -> (Rc<RefCell<Vga>>, IoBus) {
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    let io = IoBus::new(vga.clone(), |io| Vga::configure_io(io, &vga));
    (vga, io)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn register_and_dispi_operations_keep_their_distinct_transfer_rules() {
    let (_vga, io) = connect(&ManualClock::new());
    // A VGA word writes an index and then its newly selected data register.
    io.write_port(0x3c4, 2, 0x0f02);
    assert_eq!(io.read_port(0x3c4, 2), 0x0f02);
    io.write_port(0x3c5, 1, 5);
    assert_eq!(io.read_port(0x3c5, 1), 5);
    // Status reads are specialized; their independently bound writes still
    // program Feature Control through the ordinary byte-transfer operation.
    io.write_port(0x3da, 1, 3);
    assert_eq!(io.read_port(0x3ca, 1), 3);

    // DISPI words address one complete register rather than two adjacent bytes.
    io.write_port(0x1ce, 2, 0);
    assert_eq!(io.read_port(0x1cf, 2), 0xb0c2);
    io.write_port(0x1ce, 2, 1);
    io.write_port(0x1cf, 2, 800);
    assert_eq!(io.read_port(0x1cf, 2), 800);
    for (bytes, open_bus) in [(1, 0xff), (4, u32::MAX)] {
        io.write_port(0x1ce, bytes, 2);
        io.write_port(0x1cf, bytes, 640);
        assert_eq!(io.read_port(0x1cf, bytes), open_bus);
    }
    assert_eq!(io.read_port(0x1ce, 2), 1);
    assert_eq!(io.read_port(0x1cf, 2), 800);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn individual_registers_and_wide_tails_share_access_effects() {
    let (_vga, io) = connect(&ManualClock::new());
    struct Neighbour;
    impl Device for Neighbour {}
    let neighbour_accesses = Rc::new(Cell::new(0));
    io.connect(Rc::new(RefCell::new(Neighbour)), |io| {
        for address in [0x3d0, 0x3d6] {
            let reads = neighbour_accesses.clone();
            let writes = neighbour_accesses.clone();
            io.port(
                address,
                PortPolicy::BYTE_SEQUENCE,
                move |_| {
                    reads.set(reads.get() + 1);
                    0xa5
                },
                move |_, _| writes.set(writes.get() + 1),
            );
        }
    });
    io.write_port(0x3ce, 1, 8);
    io.write_port(0x3cf, 1, 0x5a);
    assert_eq!(io.read_port(0x3ce, 2), 0x5a08);
    assert_eq!(io.read_port(0x3cf, 1), 0x5a);

    io.write_port(0x3c0, 1, 0x12);
    io.write_port(0x3c0, 1, 7);
    assert_eq!(io.read_port(0x3c0, 1), 0x12);
    assert_eq!(io.read_port(0x3c1, 1), 7);
    assert_eq!(io.read_port(0x3c2, 1), 0x10);

    io.write_port(0x3c6, 1, 0x5a);
    assert_eq!(io.read_port(0x3c6, 1), 0x5a);
    io.write_port(0x3c8, 1, 0x20);
    for component in [11, 22, 33] {
        io.write_port(0x3c9, 1, component);
    }
    assert_eq!(io.read_port(0x3c8, 1), 0x21);
    assert_eq!(io.read_port(0x3c7, 1), 0);
    io.write_port(0x3c7, 1, 0x20);
    assert_eq!(io.read_port(0x3c7, 1), 3);
    // The trailing byte reads DAC data exactly once, advancing its RGB phase.
    assert_eq!(io.read_port(0x3c8, 2), 0x0b21);
    assert_eq!(io.read_port(0x3c9, 1), 22);
    assert_eq!(io.read_port(0x3c9, 1), 33);

    // A hole or read-only first byte must not discard the remaining transfer.
    io.write_port(0x3c3, 4, 0x6b05_0200);
    assert_eq!(io.read_port(0x3c3, 4), 0x6b05_02ff);
    io.write_port(0x3cd, 4, 0x003c_0800);
    assert_eq!(io.read_port(0x3cd, 4), 0xff3c_08ff);
    // Tails stop at the VGA cluster boundary even when a neighbour is mapped.
    io.write_port(0x3d4, 1, 0x0d);
    io.write_port(0x3d5, 4, 0x1234_565a);
    assert_eq!(io.read_port(0x3d5, 4), 0xffff_ff5a);
    assert_eq!(neighbour_accesses.get(), 0);
    assert_eq!(io.read_port(0x3d0, 1), 0xa5);
    assert_eq!(io.read_port(0x3d6, 1), 0xa5);
    assert_eq!(neighbour_accesses.get(), 2);
    io.write_port(0x3c1, 2, 0x4200);
    assert_eq!(io.read_port(0x3cb, 2), 0x42ff);
    assert_eq!(io.read_port(0x3cc, 1), 0x42);
    assert_eq!(io.read_port(0x3da, 1), 0xff);
    assert_eq!(io.read_port(0x3ba, 1), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn status_routing_and_attribute_phase_follow_writes_and_state_restore() {
    let (vga, io) = connect(&ManualClock::new());
    io.write_port(0x3d4, 1, 0x0d);
    io.write_port(0x3d5, 1, 0x23);
    // Reselecting the active CRTC writer must leave the other data alias disabled.
    io.write_port(0x3b5, 1, 0x67);
    assert_eq!(io.read_port(0x3d4, 2), 0x230d);
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    io.write_port(0x3c0, 1, 0x12); // Leave the attribute controller expecting data.
    assert_eq!(io.read_port(0x3ba, 1), 0xff);
    io.write_port(0x3c0, 1, 7);
    assert_eq!(vga.borrow().registers().attribute.values[0x12], 7);

    for (bytes, expected) in [(1, 0), (2, 0xff00), (4, 0xffff_ff00)] {
        io.write_port(0x3c0, 1, 0x12);
        assert_eq!(io.read_port(0x3da, bytes), expected);
        assert_eq!(vga.borrow().core_state().to_bytes()[8], 0);
    }

    // A dword starting at the attribute port reaches Misc Output as its third
    // byte. Its new mono alias must be effective when the transfer returns.
    io.write_port(0x3c0, 4, 0x0042_0012);
    let mono = vga.borrow().core_state();
    assert_eq!(io.read_port(0x3b4, 1), 0x0d);
    assert_eq!(io.read_port(0x3b5, 1), 0x23);
    io.write_port(0x3b4, 1, 0x0c);
    io.write_port(0x3b4, 1, 0x0d); // Also reselect through the monochrome index callback.
    io.write_port(0x3d4, 2, 0x450c); // Inactive CRTC writes are ignored.
    io.write_port(0x3d5, 1, 0x67);
    assert_eq!(io.read_port(0x3b4, 2), 0x230d);
    io.write_port(0x3ba, 1, 3);
    io.write_port(0x3da, 1, 7); // Inactive Feature Control writes are ignored.
    assert_eq!(io.read_port(0x3ca, 1), 3);
    assert_eq!(mono.to_bytes()[8], 1);
    assert_eq!(io.read_port(0x3da, 4), u32::MAX);
    assert_eq!(vga.borrow().core_state().to_bytes()[8], 1);
    assert_eq!(io.read_port(0x3ba, 4), 0xffff_ff00);

    io.write_port(0x46e8, 1, 0);
    let disabled = vga.borrow().core_state();
    assert_eq!(io.read_port(0x3ba, 1), 0xff);
    assert_eq!(io.read_port(0x3b4, 2), 0xffff);
    assert_eq!(io.read_port(0x3c4, 2), 0xffff);
    assert_eq!(io.read_port(0x1ce, 2), 0xffff);
    io.write_port(0x3c4, 2, 0x0002);
    io.write_port(0x1ce, 2, 1);
    assert_eq!(vga.borrow().core_state().to_bytes(), disabled.to_bytes());
    io.with_context(|context| {
        vga.borrow_mut()
            .restore_state(context, Some(mono), None, None);
    });
    assert_eq!(io.read_port(0x3da, 1), 0xff);
    assert_eq!(vga.borrow().core_state().to_bytes()[8], 1);
    assert_eq!(io.read_port(0x3ba, 2), 0xff00);
    assert_eq!(io.read_port(0x3b4, 2), 0x230d);
    assert_eq!(io.read_port(0x1ce, 2), 0);
    io.write_port(0x3c0, 1, 0x13);
    assert_eq!(vga.borrow().registers().attribute.address(), 0x13);
    io.with_context(|context| {
        vga.borrow_mut()
            .restore_state(context, Some(disabled), None, None);
    });
    assert_eq!(io.read_port(0x3ba, 1), 0xff);

    // Setup mode and Option Select jointly control address decoding.
    for (control, option, expected) in [(0x18, 0, 0xff), (8, 1, 0xff), (0x18, 1, 0xff)] {
        io.write_port(0x46e8, 1, control);
        io.write_port(0x102, 1, option);
        assert_eq!(
            io.read_port(0x102, 1),
            if control == 0x18 { option } else { 0xff }
        );
        assert_eq!(io.read_port(0x3ba, 1), expected);
    }
    io.write_port(0x46e8, 1, 8);
    assert_eq!(io.read_port(0x3ba, 1), 0);
    io.write_port(0x3c2, 1, 0x43);
    assert_eq!(io.read_port(0x3ba, 1), 0xff);
    assert_eq!(io.read_port(0x3da, 1), 0);
}

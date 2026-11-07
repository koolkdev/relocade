#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

fn poll(pic: &mut Pic) -> Poll<u8> {
    pic.poll_interrupt(&mut Context::from_waker(Waker::noop()))
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn masking_preserves_edges_and_eoi_releases_lower_priority_requests() {
    let mut pic = Pic::new(8, 0x70);
    let irq1 = pic.irq(1, false);
    irq1.pulse();
    let irq0 = pic.irq(0, true);
    irq0.pulse();
    assert_eq!(poll(&mut pic), Poll::Pending);
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 3);
    pic.write_port(MASK_PORT, 1, 0xfc); // Unmask IRQ0 and IRQ1.
    assert_eq!(poll(&mut pic), Poll::Ready(8));
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(COMMAND_PORT, 1, 0x0b); // OCW3: read the in-service register.
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 1);
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    irq0.pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(8), "higher priority may nest");
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 3);
    pic.write_port(COMMAND_PORT, 1, 0x61); // OCW2: specific EOI for IRQ1.
    assert_eq!(
        pic.read_port(COMMAND_PORT, 1),
        1,
        "specific EOI clears only IRQ1"
    );
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn edge_inputs_require_a_new_rising_transition_and_coalesce() {
    let mut pic = Pic::new(8, 0x70);
    pic.write_port(MASK_PORT, 1, 0xfd); // Unmask IRQ1.
    let line = pic.irq(1, true);
    assert_eq!(poll(&mut pic), Poll::Pending);
    line.set_level(true);
    assert_eq!(poll(&mut pic), Poll::Pending);
    line.set_level(false);
    line.set_level(true);
    line.set_level(true);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "a held line is not another edge"
    );
    line.pulse();
    line.pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(poll(&mut pic), Poll::Pending);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn initialization_remaps_vectors_and_auto_eoi_releases_service() {
    let mut pic = Pic::new(8, 0x70);
    let irq1 = pic.irq(1, false);
    pic.write_port(COMMAND_PORT, 1, 0x11); // ICW1: cascaded, edge-triggered, ICW4 follows.
    pic.write_port(MASK_PORT, 1, 0x20); // ICW2: vector base 20h.
    pic.write_port(MASK_PORT, 1, 4); // ICW3: slave wired to IRQ2.
    irq1.pulse();
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "finish initialization before delivery"
    );
    pic.write_port(MASK_PORT, 1, 3); // ICW4: 8086 mode with automatic EOI.
    assert_eq!(poll(&mut pic), Poll::Ready(0x21));
    irq1.pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(0x21));
    pic.write_port(COMMAND_PORT, 1, 0x0b); // OCW3: read the in-service register.
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn level_requests_remain_asserted_until_the_device_lowers_them() {
    let mut pic = Pic::new(8, 0x70);
    let line = pic.irq(1, true);
    for (port, value) in [
        (COMMAND_PORT, 0x1b), // ICW1: level-triggered, single, ICW4 follows.
        (MASK_PORT, 8),       // ICW2: BIOS vector base.
        (MASK_PORT, 1),       // ICW4: 8086 mode, software EOI.
    ] {
        pic.write_port(port, 1, value);
    }
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    line.set_level(false);
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(poll(&mut pic), Poll::Pending);

    let irq0 = pic.irq(0, true);
    assert_eq!(poll(&mut pic), Poll::Ready(8));
    irq0.set_level(false);
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.

    line.set_level(true);
    for (port, value) in [
        (COMMAND_PORT, 0x13), // ICW1: edge-triggered, single, ICW4 follows.
        (MASK_PORT, 8),       // ICW2: BIOS vector base.
        (MASK_PORT, 1),       // ICW4: 8086 mode, software EOI.
    ] {
        pic.write_port(port, 1, value);
    }
    line.set_level(true);
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "initialization does not create a physical edge"
    );
    line.set_level(false);
    line.set_level(true);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rotation_polling_and_special_mask_use_the_same_arbitration() {
    let mut pic = Pic::new(8, 0x70);
    let irq1 = pic.irq(1, false);
    let irq0 = pic.irq(0, false);
    pic.write_port(MASK_PORT, 1, 0);
    irq0.pulse();
    irq1.pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(8));
    pic.write_port(COMMAND_PORT, 1, 0xa0); // OCW2: non-specific EOI and rotate priority.
    irq0.pulse();
    pic.write_port(COMMAND_PORT, 1, 0x0c); // OCW3: poll and acknowledge on the next read.
    assert_eq!(
        pic.read_port(COMMAND_PORT, 1),
        0x81,
        "poll acknowledges IRQ1 after rotation"
    );
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(MASK_PORT, 1, 2); // Mask the in-service IRQ1.
    pic.write_port(COMMAND_PORT, 1, 0x68); // OCW3: enable special mask mode.
    assert_eq!(
        poll(&mut pic),
        Poll::Ready(8),
        "masked service stops blocking in special mask mode"
    );
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    pic.write_port(COMMAND_PORT, 1, 0x0b); // OCW3: read the in-service register.
    assert_eq!(
        pic.read_port(COMMAND_PORT, 1),
        2,
        "non-specific EOI skips masked service"
    );
}

#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn masked_input_wakes_a_waiting_cpu_when_unmasked() {
    let mut pic = Pic::new(8, 0x70);
    let irq1 = pic.irq(1, false);
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pic
        .poll_interrupt(&mut Context::from_waker(&waker))
        .is_pending());
    irq1.pulse();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    pic.write_port(MASK_PORT, 1, 0xfd); // Unmask IRQ1.
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    irq1.pulse();
    assert!(pic
        .poll_interrupt(&mut Context::from_waker(&waker))
        .is_pending());
    pic.write_port(COMMAND_PORT, 1, 0x20); // OCW2: non-specific EOI.
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn slave_irq_requires_both_masks_and_both_eois() {
    let mut pic = Pic::default();
    let mouse = pic.irq(MOUSE_IRQ, false);
    let timer = pic.irq(TIMER_IRQ, false);
    mouse.pulse();
    pic.unmask_irq(MOUSE_IRQ);
    assert_eq!(poll(&mut pic), Poll::Pending);
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 4);
    pic.unmask_irq(CASCADE_IRQ);
    assert_eq!(poll(&mut pic), Poll::Ready(0x74));
    for port in [COMMAND_PORT, SLAVE_COMMAND_PORT] {
        pic.write_port(port, 1, 0x0b);
    }
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 4);
    assert_eq!(pic.read_port(SLAVE_COMMAND_PORT, 1), 0x10);
    pic.unmask_irq(TIMER_IRQ);
    timer.pulse();
    assert_eq!(
        poll(&mut pic),
        Poll::Ready(8),
        "timer can preempt the cascade"
    );
    pic.write_port(COMMAND_PORT, 1, 0x60); // Specific EOI for timer only.
    mouse.pulse();
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Pending, "master ISR2 still blocks");
    pic.write_port(COMMAND_PORT, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Ready(0x74));
    mouse.pulse();
    pic.write_port(COMMAND_PORT, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Pending, "slave ISR4 still blocks");
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Ready(0x74));
}

fn initialize_chip(pic: &mut Pic, command: u16, base: u8, cascade: u8, mode: u8) {
    pic.write_port(command, 1, 0x11);
    for value in [base, cascade, mode] {
        pic.write_port(command + 1, 1, value.into());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn slave_remapping_auto_eoi_and_polling_share_chip_arbitration() {
    let mut pic = Pic::default();
    initialize_chip(&mut pic, COMMAND_PORT, 0x20, 4, 3);
    initialize_chip(&mut pic, SLAVE_COMMAND_PORT, 0x28, 2, 3);
    pic.irq(12, false).pulse();
    pic.irq(13, false).pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(0x2c));
    assert_eq!(poll(&mut pic), Poll::Ready(0x2d), "INTA rearms the cascade");
    assert_eq!(poll(&mut pic), Poll::Pending);

    initialize_chip(&mut pic, COMMAND_PORT, 8, 4, 1);
    initialize_chip(&mut pic, SLAVE_COMMAND_PORT, 0x70, 2, 1);
    pic.irq(12, false).pulse();
    pic.write_port(COMMAND_PORT, 1, 0x0c);
    assert_eq!(pic.read_port(COMMAND_PORT, 1), 0x82);
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x0c);
    assert_eq!(pic.read_port(SLAVE_COMMAND_PORT, 1), 0x84);
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x0b);
    assert_eq!(pic.read_port(SLAVE_COMMAND_PORT, 1), 0x10);
    assert_eq!(poll(&mut pic), Poll::Pending);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn masking_a_latched_slave_request_produces_spurious_irq15() {
    let mut pic = Pic::default();
    pic.unmask_irq(CASCADE_IRQ);
    pic.unmask_irq(MOUSE_IRQ);
    pic.irq(MOUSE_IRQ, false).pulse();
    pic.write_port(SLAVE_MASK_PORT, 1, 0xff);
    assert_eq!(poll(&mut pic), Poll::Ready(0x77));
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x0b);
    assert_eq!(pic.read_port(SLAVE_COMMAND_PORT, 1), 0);
    pic.write_port(COMMAND_PORT, 1, 0x20);
    pic.unmask_irq(MOUSE_IRQ);
    assert_eq!(
        poll(&mut pic),
        Poll::Ready(0x74),
        "masking retained the real request"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn slave_unmask_and_eoi_wake_the_waiting_cpu() {
    let mut pic = Pic::default();
    pic.unmask_irq(CASCADE_IRQ);
    let mouse = pic.irq(MOUSE_IRQ, false);
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(pic.poll_interrupt(&mut cx).is_pending());
    mouse.pulse();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    pic.unmask_irq(MOUSE_IRQ);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(pic.poll_interrupt(&mut cx), Poll::Ready(0x74));
    mouse.pulse();
    assert!(pic.poll_interrupt(&mut cx).is_pending());
    pic.write_port(SLAVE_COMMAND_PORT, 1, 0x20);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    pic.write_port(COMMAND_PORT, 1, 0x20);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
}

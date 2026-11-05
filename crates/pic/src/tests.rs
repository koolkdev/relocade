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

#[test]
fn masking_preserves_edges_and_eoi_releases_lower_priority_requests() {
    let mut pic = Pic::new(8);
    let irq1 = pic.irq(1).connect(false);
    irq1.pulse();
    let irq0 = pic.irq(0).connect(true);
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

#[test]
fn edge_inputs_require_a_new_rising_transition_and_coalesce() {
    let mut pic = Pic::new(8);
    pic.write_port(MASK_PORT, 1, 0xfd); // Unmask IRQ1.
    let line = pic.irq(1).connect(true);
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

#[test]
fn initialization_remaps_vectors_and_auto_eoi_releases_service() {
    let mut pic = Pic::new(8);
    let irq1 = pic.irq(1).connect(false);
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

#[test]
fn level_requests_remain_asserted_until_the_device_lowers_them() {
    let mut pic = Pic::new(8);
    let line = pic.irq(1).connect(true);
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

    let irq0 = pic.irq(0).connect(true);
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

#[test]
fn rotation_polling_and_special_mask_use_the_same_arbitration() {
    let mut pic = Pic::new(8);
    let irq1 = pic.irq(1).connect(false);
    let irq0 = pic.irq(0).connect(false);
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

#[test]
fn masked_input_wakes_a_waiting_cpu_when_unmasked() {
    let mut pic = Pic::new(8);
    let irq1 = pic.irq(1).connect(false);
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

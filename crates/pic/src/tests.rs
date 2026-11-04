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
    pic.irq(1).pulse();
    pic.irq(0).pulse();
    assert_eq!(poll(&mut pic), Poll::Pending);
    assert_eq!(pic.read_port(0x20, 1), 3);
    pic.write_port(0x21, 1, 0xfc);
    assert_eq!(poll(&mut pic), Poll::Ready(8));
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(0x20, 1, 0x0b);
    assert_eq!(pic.read_port(0x20, 1), 1);
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.irq(0).pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(8), "higher priority may nest");
    assert_eq!(pic.read_port(0x20, 1), 3);
    pic.write_port(0x20, 1, 0x61);
    assert_eq!(pic.read_port(0x20, 1), 1, "specific EOI clears only IRQ1");
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(pic.read_port(0x20, 1), 0);
}

#[test]
fn edge_inputs_require_a_new_rising_transition_and_coalesce() {
    let mut pic = Pic::new(8);
    pic.write_port(0x21, 1, 0xfd);
    let line = pic.irq(1);
    line.set_level(true);
    line.set_level(true);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "a held line is not another edge"
    );
    line.pulse();
    line.pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Pending);
}

#[test]
fn initialization_remaps_vectors_and_auto_eoi_releases_service() {
    let mut pic = Pic::new(8);
    pic.write_port(0x20, 1, 0x11);
    pic.write_port(0x21, 1, 0x20);
    pic.write_port(0x21, 1, 4);
    pic.irq(1).pulse();
    assert_eq!(
        poll(&mut pic),
        Poll::Pending,
        "finish initialization before delivery"
    );
    pic.write_port(0x21, 1, 3);
    assert_eq!(poll(&mut pic), Poll::Ready(0x21));
    pic.irq(1).pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(0x21));
    pic.write_port(0x20, 1, 0x0b);
    assert_eq!(pic.read_port(0x20, 1), 0);
}

#[test]
fn level_requests_remain_asserted_until_the_device_lowers_them() {
    let mut pic = Pic::new(8);
    let line = pic.irq(1);
    line.set_level(true);
    for (port, value) in [(0x20, 0x1b), (0x21, 8), (0x21, 1)] {
        pic.write_port(port, 1, value);
    }
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    line.set_level(false);
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(poll(&mut pic), Poll::Pending);

    line.set_level(true);
    for (port, value) in [(0x20, 0x13), (0x21, 8), (0x21, 1)] {
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
    pic.write_port(0x21, 1, 0);
    pic.irq(0).pulse();
    pic.irq(1).pulse();
    assert_eq!(poll(&mut pic), Poll::Ready(8));
    pic.write_port(0x20, 1, 0xa0);
    pic.irq(0).pulse();
    pic.write_port(0x20, 1, 0x0c);
    assert_eq!(
        pic.read_port(0x20, 1),
        0x81,
        "poll acknowledges IRQ1 after rotation"
    );
    assert_eq!(poll(&mut pic), Poll::Pending);
    pic.write_port(0x21, 1, 2);
    pic.write_port(0x20, 1, 0x68);
    assert_eq!(
        poll(&mut pic),
        Poll::Ready(8),
        "masked service stops blocking in special mask mode"
    );
    pic.write_port(0x20, 1, 0x20);
    pic.write_port(0x20, 1, 0x0b);
    assert_eq!(
        pic.read_port(0x20, 1),
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
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pic
        .poll_interrupt(&mut Context::from_waker(&waker))
        .is_pending());
    pic.irq(1).pulse();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    pic.write_port(0x21, 1, 0xfd);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(poll(&mut pic), Poll::Ready(9));
    pic.irq(1).pulse();
    assert!(pic
        .poll_interrupt(&mut Context::from_waker(&waker))
        .is_pending());
    pic.write_port(0x20, 1, 0x20);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
}

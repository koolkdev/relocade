use relocade_machine::IoBus;
use relocade_parallel::{InputPins, Inputs, OutputPins, Port};
use relocade_pic::Pic;
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn hardware() -> (IoBus, Rc<RefCell<Port>>) {
    let pic = Pic::default();
    let port = Rc::new(RefCell::new(Port::new(0x378, pic.irq(7, false))));
    let pic = Rc::new(RefCell::new(pic));
    let io = IoBus::new(pic.clone(), |io| Pic::configure_io(io, &pic));
    io.connect(port.clone(), |io| Port::configure_io(io, &port));
    (io, port)
}

fn attach(port: &Rc<RefCell<Port>>) -> Inputs {
    let captured = Rc::new(RefCell::new(None));
    port.borrow_mut().attach({
        let captured = captured.clone();
        move |_, inputs| *captured.borrow_mut() = Some(inputs.clone())
    });
    let inputs = captured.borrow().clone().unwrap();
    inputs
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn latches_drive_physical_pins_without_a_handshake() {
    let (io, port) = hardware();
    assert_eq!(io.read_port(0x378, 1), 0);
    assert_eq!(io.read_port(0x379, 1), 0x7f);
    assert_eq!(io.read_port(0x37a, 1), 0xe0);
    assert_eq!(
        port.borrow().output_pins(),
        OutputPins {
            data: 0,
            strobe_n: true,
            auto_feed_n: true,
            initialize_n: false,
            select_in_n: true,
        }
    );
    io.write_port(0x378, 1, 0xa5);
    assert_eq!(io.read_port(0x378, 1), 0xa5);
    assert!(port.borrow().output_pins().strobe_n);
    assert_eq!(io.read_port(0x379, 1), 0x7f);
    io.write_port(0x37a, 1, 0xef);
    assert_eq!(io.read_port(0x37a, 1), 0xef);
    assert_eq!(
        port.borrow().output_pins(),
        OutputPins {
            data: 0xa5,
            strobe_n: false,
            auto_feed_n: false,
            initialize_n: true,
            select_in_n: false,
        }
    );
    io.write_port(0x37a, 1, 4);
    assert!(port.borrow().output_pins().strobe_n);
    assert!(port.borrow().output_pins().auto_feed_n);
    assert!(port.borrow().output_pins().select_in_n);
    assert_eq!(io.read_port(0x379, 1), 0x7f);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn status_reflects_external_levels_and_only_busy_is_inverted() {
    let (io, port) = hardware();
    let driver = attach(&port);
    let mut inputs = InputPins {
        busy: false,
        ..Default::default()
    };
    driver.set(inputs);
    assert_eq!(io.read_port(0x379, 1), 0xff);
    inputs.acknowledge_n = false;
    driver.set(inputs);
    assert_eq!(io.read_port(0x379, 1), 0xbf);
    inputs.paper_end = false;
    driver.set(inputs);
    assert_eq!(io.read_port(0x379, 1), 0x9f);
    inputs.selected = false;
    driver.set(inputs);
    assert_eq!(io.read_port(0x379, 1), 0x8f);
    inputs.error_n = false;
    driver.set(inputs);
    assert_eq!(io.read_port(0x379, 1), 0x87);
    io.write_port(0x379, 1, 0xff);
    io.write_port(0x378, 2, 0x1234);
    io.write_port(0x37a, 4, 0x0c);
    assert_eq!(io.read_port(0x379, 1), 0x87);
    assert_eq!(io.read_port(0x378, 1), 0);
    assert_eq!(io.read_port(0x37a, 1), 0xe0);
    assert_eq!(io.read_port(0x378, 2), 0xffff);
    assert_eq!(io.read_port(0x37a, 4), u32::MAX);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn acknowledge_irq_is_gated_before_pic_masking_and_edge_detection() {
    let (io, port) = hardware();
    let poll = || io.poll_interrupt(&mut Context::from_waker(Waker::noop()));
    let driver = attach(&port);
    let mut inputs = InputPins {
        acknowledge_n: false,
        ..Default::default()
    };
    driver.set(inputs);
    inputs.acknowledge_n = true;
    driver.set(inputs);
    io.write_port(0x21, 1, 0x7f); // Unmask IRQ7; the disabled gate produced no edge.
    assert_eq!(poll(), Poll::Pending);
    io.write_port(0x21, 1, 0xff);
    io.write_port(0x37a, 1, 0x10); // Enabling with ACK high raises the IRQ line.
    assert_eq!(poll(), Poll::Pending);
    io.write_port(0x21, 1, 0x7f);
    assert_eq!(poll(), Poll::Ready(0x0f));
    io.write_port(0x20, 1, 0x20);
    driver.set(inputs);
    assert_eq!(poll(), Poll::Pending);
    inputs.acknowledge_n = false;
    driver.set(inputs);
    assert_eq!(poll(), Poll::Pending);
    inputs.acknowledge_n = true;
    driver.set(inputs);
    assert_eq!(poll(), Poll::Ready(0x0f));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn output_notifications_preserve_edges_and_can_drive_inputs() {
    let (io, port) = hardware();
    let seen = Rc::new(RefCell::new(Vec::new()));
    port.borrow_mut().attach({
        let seen = seen.clone();
        move |outputs, inputs| {
            seen.borrow_mut().push(outputs);
            inputs.set(InputPins {
                busy: !outputs.strobe_n,
                ..Default::default()
            });
        }
    });
    io.write_port(0x378, 1, 0x41);
    io.write_port(0x37a, 1, 0x0d);
    assert_eq!(io.read_port(0x379, 1), 0x7f);
    io.write_port(0x378, 1, 0x42);
    io.write_port(0x37a, 1, 0x0c);
    assert_eq!(io.read_port(0x379, 1), 0xff);
    io.write_port(0x37a, 1, 0x0c); // Unchanged outputs do not create edges.
    io.write_port(0x37a, 1, 0x1c); // IRQ gating does not change connector outputs.
    let seen = seen.borrow();
    assert_eq!(seen.len(), 5);
    assert_eq!(seen[0].data, 0);
    assert_eq!(seen[1].data, 0x41);
    assert!(seen[1].strobe_n);
    assert!(!seen[2].strobe_n);
    assert_eq!(seen[2].data, 0x41);
    assert!(!seen[3].strobe_n);
    assert_eq!(seen[3].data, 0x42);
    assert!(seen[4].strobe_n);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn readiness_waits_for_busy_release_and_cancels_without_retaining_a_listener() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::Wake;

    #[derive(Default)]
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let (io, port) = hardware();
    let driver = attach(&port);

    let mut canceled = Box::pin(port.borrow().wait_ready());
    assert!(canceled.as_mut().poll(&mut cx).is_pending());
    drop(canceled);
    driver.set(InputPins {
        busy: false,
        ..Default::default()
    });
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);

    driver.set(InputPins::default());
    let mut ready = pin!(port.borrow().wait_ready());
    assert!(ready.as_mut().poll(&mut cx).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    driver.set(InputPins {
        acknowledge_n: false,
        ..Default::default()
    });
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    driver.set(InputPins {
        busy: false,
        acknowledge_n: false,
        ..Default::default()
    });
    assert!(wakes.0.load(Ordering::SeqCst) > 0);
    assert!(ready.as_mut().poll(&mut cx).is_ready());
    assert_eq!(io.read_port(0x379, 1), 0xbf);
    assert_eq!(io.read_port(0x378, 1), 0);
    assert_eq!(io.read_port(0x37a, 1), 0xe0);

    // Readiness is checked when polled, even if BUSY changed before registration.
    driver.set(InputPins::default());
    let ready = port.borrow().wait_ready();
    driver.set(InputPins {
        busy: false,
        ..Default::default()
    });
    assert!(pin!(ready).poll(&mut cx).is_ready());
}

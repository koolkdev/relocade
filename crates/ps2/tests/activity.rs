use relocade_frontend::keyboard::{
    ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, ModifiersState,
};
use relocade_machine::{worker::wake_connection, Clock, CpuStorage, IoBus, Runner};
use relocade_pic::{end_of_interrupt, Pic, KEYBOARD_IRQ, PORT_RANGE};
use relocade_ps2::{
    connect, read_keyboard_latch, DATA_PORT, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
};
use std::{
    cell::{Cell, RefCell},
    future::{pending, poll_fn},
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, Default)]
struct ManualClock(Rc<Time>);

#[derive(Default)]
struct Time {
    now: Cell<Duration>,
    waiter: RefCell<Option<Waker>>,
}

impl ManualClock {
    fn advance(&self, millis: u64) {
        let now = Duration::from_millis(millis);
        assert!(now >= self.now());
        self.0.now.set(now);
        let waiter = self.0.waiter.borrow_mut().take();
        if let Some(waiter) = waiter {
            waiter.wake();
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

fn key(code: KeyCode, repeat: bool) -> KeyboardEvent {
    KeyboardEvent::Key {
        physical_key: code.into(),
        logical_key: Key::Character("ש".into()),
        location: KeyLocation::Standard,
        text: Some("ש".into()),
        state: ElementState::Pressed,
        repeat,
    }
}

async fn next_byte(io: &IoBus) -> u8 {
    for _ in 0..32 {
        if io.poll_interrupt(&mut Context::from_waker(Waker::noop())) == Poll::Ready(9) {
            let byte = read_keyboard_latch(io).expect("IRQ1 must expose a keyboard byte");
            end_of_interrupt(io);
            return byte;
        }
        relocade_machine::yield_now().await;
    }
    panic!("keyboard activity did not publish an IRQ byte");
}

async fn no_output(io: &IoBus) {
    // Let the activity process queued notifications before checking for output.
    for _ in 0..4 {
        relocade_machine::yield_now().await;
    }
    assert_eq!(
        io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(OUTPUT_BUFFER_FULL),
        0
    );
    assert_eq!(
        io.poll_interrupt(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
}

async fn connected_activity() {
    let clock = ManualClock::default();
    let storage = CpuStorage::new().unwrap();
    let (_host, endpoint) = wake_connection(storage.interrupt_handle());
    let runner = Runner::new(clock.clone(), storage, endpoint);
    let result = runner
        .run(pending::<()>(), async |runner| {
            let pic = Pic::default();
            pic.unmask_irq(KEYBOARD_IRQ);
            let (inputs, keyboard) = connect(runner, &pic).await;
            let input = inputs.keyboard;
            let pic = runner
                .connect(Rc::new(RefCell::new(pic)), [], [PORT_RANGE])
                .await;
            let io = IoBus::new([pic, keyboard]);
            io.write_port(DATA_PORT, 1, 0xf3); // Program typematic delay/rate.
            assert_eq!(next_byte(&io).await, 0xfa);
            io.write_port(DATA_PORT, 1, 0); // 250ms delay, 30Hz.
            assert_eq!(next_byte(&io).await, 0xfa);
            input.unbounded_send(key(KeyCode::KeyA, false)).unwrap();
            assert_eq!(next_byte(&io).await, 0x1e);
            clock.advance(249);
            no_output(&io).await;
            clock.advance(250);
            assert_eq!(next_byte(&io).await, 0x1e);
            clock.advance(4000);
            assert_eq!(next_byte(&io).await, 0x1e);
            no_output(&io).await; // Late scheduling produces one repeat, not a burst.
            input.unbounded_send(KeyboardEvent::FocusLost).unwrap();
            assert_eq!(next_byte(&io).await, 0x9e);
            clock.advance(5000);
            no_output(&io).await;

            input
                .unbounded_send(KeyboardEvent::Modifiers(ModifiersState::SHIFT))
                .unwrap();
            assert_eq!(next_byte(&io).await, 0x2a);
            input.unbounded_send(key(KeyCode::KeyA, false)).unwrap();
            assert_eq!(next_byte(&io).await, 0x1e);
            input.unbounded_send(key(KeyCode::KeyA, true)).unwrap();
            input.unbounded_send(key(KeyCode::KeyB, false)).unwrap();
            assert_eq!(next_byte(&io).await, 0x30, "host repeat was suppressed");
            input.unbounded_send(KeyboardEvent::FocusLost).unwrap();
            let mut releases = [
                next_byte(&io).await,
                next_byte(&io).await,
                next_byte(&io).await,
            ];
            releases.sort();
            assert_eq!(releases, [0x9e, 0xaa, 0xb0]);
            no_output(&io).await;
        })
        .await;
    assert_eq!(result, Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn connected_keyboard_delivers_events_and_clock_driven_repeats() {
    pollster::block_on(connected_activity());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn connected_keyboard_delivers_events_and_clock_driven_repeats() {
    connected_activity().await;
}

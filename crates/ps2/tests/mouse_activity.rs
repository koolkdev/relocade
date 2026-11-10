use relocade_clock_test_support::ManualClock;
use relocade_frontend::input::InputEvent;
use relocade_frontend::{
    keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent},
    mouse::{MouseButton, MouseEvent},
};
use relocade_machine::{worker::wake_connection, CpuStorage, IoBus, Runner};
use relocade_pic::{Pic, CASCADE_IRQ, KEYBOARD_IRQ, MOUSE_IRQ};
use relocade_ps2::{
    read_keyboard_latch, AUXILIARY_OUTPUT, DATA_PORT, INPUT_BUFFER_FULL, STATUS_COMMAND_PORT,
};
use std::{
    future::pending,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn write(io: &IoBus, port: u16, value: u8) {
    for _ in 0..64 {
        if io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(INPUT_BUFFER_FULL) == 0 {
            io.write_port(port, 1, value.into());
            return;
        }
        relocade_machine::yield_now().await;
    }
    panic!("PS/2 input register stayed busy");
}

async fn next(io: &IoBus) -> (bool, u8) {
    for _ in 0..64 {
        if let Poll::Ready(vector) = io.poll_interrupt(&mut Context::from_waker(Waker::noop())) {
            let auxiliary = io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(AUXILIARY_OUTPUT) != 0;
            assert_eq!(vector, if auxiliary { 0x74 } else { 9 });
            let byte = if auxiliary {
                assert_eq!(
                    read_keyboard_latch(io),
                    None,
                    "keyboard reader must leave mouse output intact"
                );
                io.read_port(DATA_PORT, 1) as u8
            } else {
                read_keyboard_latch(io).unwrap()
            };
            relocade_pic::end_of_interrupt(
                io,
                if auxiliary {
                    relocade_pic::MOUSE_IRQ
                } else {
                    relocade_pic::KEYBOARD_IRQ
                },
            );
            return (auxiliary, byte);
        }
        relocade_machine::yield_now().await;
    }
    panic!("connected PS/2 activity did not produce an interrupt");
}

async fn connected_mouse_and_keyboard() {
    let clock = ManualClock::default();
    let storage = CpuStorage::new().unwrap();
    let (_host, endpoint) = wake_connection(storage.interrupt_handle());
    let runner = Runner::new(clock.clock(), storage, endpoint);
    let result = runner
        .run(pending::<()>(), async |runner| {
            let (pic, pic_device) = Pic::initialize(&runner).await;
            for irq in [KEYBOARD_IRQ, CASCADE_IRQ, MOUSE_IRQ] {
                pic.unmask_irq(irq);
            }
            let (input, _controller, ps2) = relocade_ps2::connect(&runner, &pic).await;
            let io = IoBus::new([pic_device, ps2]);
            write(&io, STATUS_COMMAND_PORT, 0x60).await;
            write(&io, DATA_PORT, 0x47).await;
            write(&io, STATUS_COMMAND_PORT, 0xd4).await;
            write(&io, DATA_PORT, 0xf4).await;
            assert_eq!(next(&io).await, (true, 0xfa));
            input
                .mouse
                .unbounded_send(InputEvent::new(MouseEvent::Button {
                    button: MouseButton::Left,
                    state: ElementState::Pressed,
                }))
                .unwrap();
            input
                .mouse
                .unbounded_send(InputEvent::new(MouseEvent::Motion {
                    delta_x: 2.0,
                    delta_y: -3.0,
                }))
                .unwrap();
            input
                .keyboard
                .unbounded_send(InputEvent::new(KeyboardEvent::Key {
                    physical_key: KeyCode::KeyA.into(),
                    logical_key: Key::Character("a".into()),
                    location: KeyLocation::Standard,
                    text: Some("a".into()),
                    state: ElementState::Pressed,
                    repeat: false,
                }))
                .unwrap();
            for _ in 0..4 {
                relocade_machine::yield_now().await;
            }
            clock.advance_to(clock.at(Duration::from_millis(10)));
            for (packet, key) in [([9, 2, 3], 0x1e), ([8, 0, 0], 0x9e)] {
                let mut mouse = Vec::new();
                let mut keyboard = Vec::new();
                for _ in 0..4 {
                    let (auxiliary, byte) = next(&io).await;
                    if auxiliary {
                        mouse.push(byte);
                    } else {
                        keyboard.push(byte);
                    }
                }
                assert_eq!(mouse, packet);
                assert_eq!(keyboard, [key]);
                input
                    .mouse
                    .unbounded_send(InputEvent::new(MouseEvent::FocusLost))
                    .unwrap();
                input
                    .keyboard
                    .unbounded_send(InputEvent::new(KeyboardEvent::FocusLost))
                    .unwrap();
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                clock.advance_to(clock.at(Duration::from_millis(20)));
            }
            assert_eq!(
                io.poll_interrupt(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            );
        })
        .await;
    assert_eq!(result, Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn connected_mouse_and_keyboard_keep_independent_irq_sources() {
    pollster::block_on(connected_mouse_and_keyboard());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn connected_mouse_and_keyboard_keep_independent_irq_sources() {
    connected_mouse_and_keyboard().await;
}

// Protocol tests control time directly. This source supplies input synchronization
// in the same timebase, leaving host/guest conversion to the machine clock tests.
struct InputTime {
    clock: ManualClock,
    disconnected_at: relocade_machine::Timestamp,
}

impl relocade_machine::ClockSource for InputTime {
    fn now(&self) -> relocade_machine::Timestamp {
        self.clock.clock().now()
    }
    fn sleep_until(&self, at: relocade_machine::Timestamp) -> relocade_machine::Sleep {
        self.clock.clock().sleep_until(at)
    }
    fn sync(&self, at: Option<relocade_machine::Instant>) -> relocade_machine::Sleep {
        self.sleep_until(
            at.and_then(|at| self.clock.host_timestamp(at))
                .unwrap_or(self.disconnected_at),
        )
    }
}

async fn queued_mouse_timestamps() {
    let clock = ManualClock::default();
    let start = clock.host_now();
    let storage = CpuStorage::new().unwrap();
    let (_host, endpoint) = wake_connection(storage.interrupt_handle());
    let runner = Runner::new(
        relocade_machine::Clock::new(InputTime {
            clock: clock.clone(),
            disconnected_at: clock.at(Duration::from_millis(25)),
        }),
        storage,
        endpoint,
    );
    assert_eq!(
        runner
            .run(pending::<()>(), async |runner| {
                let (pic, pic_device) = Pic::initialize(&runner).await;
                for irq in [CASCADE_IRQ, MOUSE_IRQ] {
                    pic.unmask_irq(irq);
                }
                let (input, _controller, ps2) = relocade_ps2::connect(&runner, &pic).await;
                let io = IoBus::new([pic_device, ps2]);
                write(&io, STATUS_COMMAND_PORT, 0x60).await;
                write(&io, DATA_PORT, 0x47).await;
                write(&io, STATUS_COMMAND_PORT, 0xd4).await;
                write(&io, DATA_PORT, 0xf4).await;
                assert_eq!(next(&io).await, (true, 0xfa));
                input
                    .mouse
                    .unbounded_send(InputEvent {
                        host_time: start + Duration::from_millis(1),
                        event: MouseEvent::Button {
                            button: MouseButton::Left,
                            state: ElementState::Pressed,
                        },
                    })
                    .unwrap();
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    Some(clock.at(Duration::from_millis(1)))
                );
                // New events arrive while the first event's sync is still pending.
                for (at, event) in [
                    (
                        2,
                        MouseEvent::Motion {
                            delta_x: 2.0,
                            delta_y: -3.0,
                        },
                    ),
                    (
                        12,
                        MouseEvent::Motion {
                            delta_x: 4.0,
                            delta_y: -1.0,
                        },
                    ),
                    (
                        13,
                        MouseEvent::Button {
                            button: MouseButton::Left,
                            state: ElementState::Released,
                        },
                    ),
                ] {
                    input
                        .mouse
                        .unbounded_send(InputEvent {
                            host_time: start + Duration::from_millis(at),
                            event,
                        })
                        .unwrap();
                }
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    Some(clock.at(Duration::from_millis(1)))
                );
                clock.advance_to(clock.at(Duration::from_millis(5)));
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    Some(clock.at(Duration::from_millis(10)))
                );
                clock.advance_to(clock.at(Duration::from_millis(10)));
                // The sample timer and each output selection cancel the input waiter,
                // but must retain the event due at 12 ms inside the persistent stream.
                for byte in [9, 2, 3] {
                    assert_eq!(next(&io).await, (true, byte));
                }
                input
                    .mouse
                    .unbounded_send(InputEvent {
                        host_time: start + Duration::from_millis(14),
                        event: MouseEvent::Motion {
                            delta_x: 1.0,
                            delta_y: 1.0,
                        },
                    })
                    .unwrap();
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    Some(clock.at(Duration::from_millis(12)))
                );
                clock.advance_to(clock.at(Duration::from_millis(14)));
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                clock.advance_to(clock.at(Duration::from_millis(20)));
                for byte in [8, 5, 0] {
                    assert_eq!(next(&io).await, (true, byte));
                }
                drop(input.mouse);
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    Some(clock.at(Duration::from_millis(25)))
                );
                clock.advance_to(clock.at(Duration::from_millis(25)));
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                assert_eq!(
                    clock.next_deadline(),
                    None,
                    "no motion remains after disconnection"
                );
            })
            .await,
        Some(())
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_mouse_input_keeps_timestamps_across_samples_and_cancelled_waiters() {
    pollster::block_on(queued_mouse_timestamps());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn queued_mouse_input_keeps_timestamps_across_samples_and_cancelled_waiters() {
    queued_mouse_timestamps().await;
}

#[path = "../../machine/tests/support/clock.rs"]
mod clock;

use clock::ManualClock;
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
            if auxiliary {
                io.write_port(relocade_pic::SLAVE_COMMAND_PORT, 1, 0x20);
            }
            relocade_pic::end_of_interrupt(io);
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
    let runner = Runner::new(clock.clone(), storage, endpoint);
    let result = runner
        .run(pending::<()>(), async |runner| {
            let (pic, pic_device) = Pic::initialize(runner).await;
            for irq in [KEYBOARD_IRQ, CASCADE_IRQ, MOUSE_IRQ] {
                pic.unmask_irq(irq);
            }
            let (input, ps2) = relocade_ps2::connect(runner, &pic).await;
            let io = IoBus::new([pic_device, ps2]);
            write(&io, STATUS_COMMAND_PORT, 0x60).await;
            write(&io, DATA_PORT, 0x47).await;
            write(&io, STATUS_COMMAND_PORT, 0xd4).await;
            write(&io, DATA_PORT, 0xf4).await;
            assert_eq!(next(&io).await, (true, 0xfa));
            input
                .mouse
                .unbounded_send(MouseEvent::Button {
                    button: MouseButton::Left,
                    state: ElementState::Pressed,
                })
                .unwrap();
            input
                .mouse
                .unbounded_send(MouseEvent::Motion {
                    delta_x: 2.0,
                    delta_y: -3.0,
                })
                .unwrap();
            input
                .keyboard
                .unbounded_send(KeyboardEvent::Key {
                    physical_key: KeyCode::KeyA.into(),
                    logical_key: Key::Character("a".into()),
                    location: KeyLocation::Standard,
                    text: Some("a".into()),
                    state: ElementState::Pressed,
                    repeat: false,
                })
                .unwrap();
            for _ in 0..4 {
                relocade_machine::yield_now().await;
            }
            clock.advance(Duration::from_millis(10));
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
                input.mouse.unbounded_send(MouseEvent::FocusLost).unwrap();
                input
                    .keyboard
                    .unbounded_send(KeyboardEvent::FocusLost)
                    .unwrap();
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
                clock.advance(Duration::from_millis(20));
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

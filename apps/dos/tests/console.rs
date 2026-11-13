#![cfg(not(target_arch = "wasm32"))]

#[path = "../../../crates/vga/tests/support/planes.rs"]
mod planes;
use futures_lite::future::or;
use planes::SnapshotPlanes;
use relocade_dos::Dos;
use relocade_frontend::input::InputEvent;
use std::{io::Read, time::Duration};

#[test]
fn echo_edits_input_and_returns_to_a_prompt_in_the_same_vga() {
    let prompt_row = 6;
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let console = prepared.run_until(std::future::pending::<()>(), async move |runner| {
        let clock = runner.clock().clone();
        let mut dos = Dos::new(runner).await.unwrap();
        use relocade_frontend::keyboard::{
            self, ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, ModifiersState, NativeKey,
        };
        let (input, events) = keyboard::channel();
        for (index, code) in [
            KeyCode::KeyE,
            KeyCode::KeyC,
            KeyCode::KeyH,
            KeyCode::KeyO,
            KeyCode::Space,
            KeyCode::KeyH,
            KeyCode::KeyE,
            KeyCode::KeyL,
            KeyCode::KeyL,
            KeyCode::KeyX,
            KeyCode::Backspace,
            KeyCode::KeyO,
            KeyCode::Enter,
            KeyCode::KeyU,
            KeyCode::KeyN,
            KeyCode::KeyK,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyW,
            KeyCode::KeyN,
            KeyCode::Enter,
            KeyCode::Enter,
        ]
        .into_iter()
        .enumerate()
        {
            let modifiers = if matches!(index, 1 | 3) {
                ModifiersState::SHIFT
            } else {
                ModifiersState::empty()
            };
            input
                .unbounded_send(InputEvent::new(KeyboardEvent::Modifiers(modifiers)))
                .unwrap();
            input
                .unbounded_send(InputEvent::new(KeyboardEvent::Key {
                    physical_key: code.into(),
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    location: KeyLocation::Standard,
                    text: Some("ש".into()),
                    state: ElementState::Pressed,
                    repeat: false,
                }))
                .unwrap();
        }
        let video = dos.vga();
        let inspect = async {
            for _ in 0..1000 {
                // Startup text addressing retains even physical plane addresses.
                if video
                    .borrow()
                    .snapshot()
                    .plane_byte(0, (prompt_row + 5) * 80 * 2)
                    == b'>'
                {
                    return;
                }
                clock.sleep(Duration::from_millis(1)).await;
            }
            panic!("console did not return to its prompt");
        };
        or(
            relocade_dos_app::run_console(dos.native_console(events)),
            inspect,
        )
        .await;
        let screen = video.borrow().snapshot();
        for (row, text) in [
            b"> eChO hello".as_slice(),
            b"hello",
            b"> unknown",
            b"Unknown command",
            b"> ",
            b"> ",
        ]
        .into_iter()
        .enumerate()
        {
            let start = (prompt_row + row) * 80 * 2;
            assert_eq!(
                screen
                    .plane_bytes(0, start..start + text.len() * 2)
                    .step_by(2)
                    .collect::<Vec<_>>(),
                text
            );
        }
        assert_eq!(
            dos.stdout().read(&mut [0; 16]).unwrap(),
            0,
            "Rust console output must not accumulate a duplicate byte stream"
        );
        assert!(
            dos.process().is_none(),
            "echo executes in Rust without launching guest code"
        );
    });
    assert_eq!(pollster::block_on(console).unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}

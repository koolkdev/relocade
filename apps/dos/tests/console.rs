#![cfg(not(target_arch = "wasm32"))]

use futures_lite::future::or;
use relocade_dos::Dos;
use std::{io::Read, time::Duration};

#[test]
fn echo_edits_input_and_returns_to_a_prompt_in_the_same_display() {
    let prompt_row = 6;
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let session = async move |runner: relocade_machine::Runner| {
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
                .unbounded_send(KeyboardEvent::Modifiers(modifiers))
                .unwrap();
            input
                .unbounded_send(KeyboardEvent::Key {
                    physical_key: code.into(),
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    location: KeyLocation::Standard,
                    text: Some("ש".into()),
                    state: ElementState::Pressed,
                    repeat: false,
                })
                .unwrap();
        }
        let video = dos.video_card().framebuffer_access();
        let frames = dos.frames();
        let cpu = dos.cpu().cpu_state();
        let memory = dos.cpu().memory();
        let before = cpu.instruction_count();
        let bios_ticks = memory.view().read_u32(0x46c).unwrap();
        let console = dos.native_console(events);
        // Observe a guest retrace before the line editor gets its first poll.
        console.write(b"X\r");
        while frames.recv().await.unwrap().vram_byte(0) != Some(b'X') {}
        assert_eq!(memory.view().read_u32(0x46c).unwrap(), bios_ticks);
        assert_eq!(cpu.instruction_count(), before);
        let inspect = async {
            // Mode 3 uses word addressing across four VGA planes.
            let last_prompt = u32::from(prompt_row + 5) * 80 * 2 * 4;
            while frames.recv().await.unwrap().vram_byte(last_prompt) != Some(b'>') {}
        };
        or(relocade_dos_app::run_console(console), inspect).await;
        assert_eq!(cpu.instruction_count(), before);
        let mut video = video.borrow_mut();
        let screen = video.framebuffer();
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
            assert_eq!(
                (0..text.len())
                    .map(|column| {
                        screen
                            .text_cell(prompt_row + row as u16, column as u16)
                            .unwrap()
                            .value() as u8
                    })
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
    };
    let console = prepared.run_until(futures_timer::Delay::new(Duration::from_secs(30)), session);
    assert_eq!(pollster::block_on(console).unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}

#![cfg(not(target_arch = "wasm32"))]

use futures_lite::future::{or, zip};
use relocade_dos::Dos;
use relocade_machine::{timer_service, Clock, Runner};
use std::{
    io::{Read, Write},
    time::Duration,
};

#[test]
fn echo_edits_input_and_returns_to_a_prompt_in_the_same_vga() {
    let prompt_row = 10;
    let prompt_column = 0;
    let (clock, timers) = timer_service();
    let console = Runner::with_clock(clock).run(async |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        dos.stdin()
            .write_all(b"eChO hellx\x08o\runknown\r\r")
            .unwrap();
        let video = dos.vga();
        let inspect = async {
            for _ in 0..1000 {
                // Startup text addressing retains even physical plane addresses.
                if video.borrow().snapshot().plane(0)[((prompt_row + 5) * 80 + prompt_column) * 2]
                    == b'>'
                {
                    return Ok::<(), relocade_dos::DosError>(());
                }
                runner.clock().sleep(Duration::from_millis(1)).await;
            }
            panic!("console did not return to its prompt");
        };
        or(relocade_dos_app::run_console(&mut dos), inspect)
            .await
            .unwrap();
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
                screen.plane(0)[start..start + text.len() * 2]
                    .iter()
                    .step_by(2)
                    .copied()
                    .collect::<Vec<_>>(),
                text
            );
        }
        // Literal CP437 cells and VGA attributes verify the ANSI path, including
        // restoring ordinary gray before input echo and command output.
        for (column, row, character, attribute) in [
            (3, 1, 0xdb, 0x0f),
            (9, 1, 0xbb, 0x09),
            (3, 2, 0xdb, 0x0b),
            (3, 4, 0xdb, 0x03),
            (3, 5, 0xdb, 0x09),
            (3, 6, 0xc8, 0x01),
            (3, 8, 0xb0, 0x0d),
            (6, 8, 0xc4, 0x05),
            (prompt_column, prompt_row, b'>', 0x0a),
            (prompt_column + 2, prompt_row, b'e', 0x07),
            (0, prompt_row + 1, b'h', 0x07),
        ] {
            let offset = (row * 80 + column) * 2;
            assert_eq!(screen.plane(0)[offset], character);
            assert_eq!(screen.plane(1)[offset], attribute);
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
    pollster::block_on(zip(console, timers.run()));
}

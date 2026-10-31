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
    let prompt_row = 6;
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
                if video.borrow().snapshot().plane(0)[(prompt_row + 5) * 80 * 2] == b'>' {
                    return;
                }
                runner.clock().sleep(Duration::from_millis(1)).await;
            }
            panic!("console did not return to its prompt");
        };
        or(relocade_dos_app::run_console(&mut dos), inspect).await;
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

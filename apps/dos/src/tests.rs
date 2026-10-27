use crate::{DosApplication, DEMO_PROGRAM};
use relocade_app::{spawn, Exit, Task};
use relocade_vga::Display;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(60);

fn start(program: &[u8]) -> (Task<u8>, mpsc::Receiver<Display>) {
    let (sender, display) = mpsc::channel();
    let task = spawn(DosApplication::new(program.to_vec()), move |display| {
        sender.send(display).unwrap();
    });
    (task, display)
}

#[test]
fn demonstration_output_remains_visible_after_execution() {
    let (task, display) = start(DEMO_PROGRAM);
    let display = display.recv_timeout(TIMEOUT).unwrap();
    assert_eq!(pollster::block_on(task).unwrap(), Exit::Completed(0));
    let screen = display.screen();
    let text: Vec<_> = screen.cells[996..1004]
        .iter()
        .map(|cell| cell.character)
        .collect();
    assert_eq!(text, b"Relocade");
    assert!(screen.cells.iter().all(|cell| cell.attribute == 0x1f));
}

#[test]
fn stopping_is_observed_between_ready_execution_slices() {
    let (task, display) = start(&[
        0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
        0x26, 0xc7, 0x06, 0, 0, b'X', 7, 0xeb, 0xfe, // Infinite loop without a host wait.
    ]);
    let display = display.recv_timeout(TIMEOUT).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while display.screen().cells[0].character != b'X' {
        assert!(Instant::now() < deadline, "program did not enter its loop");
        std::thread::sleep(Duration::from_millis(1));
    }
    task.stop();
    assert_eq!(pollster::block_on(task).unwrap(), Exit::Cancelled);
    assert_eq!(display.screen().cells[0].character, b'X');
}

#[test]
fn invalid_program_fails_before_publishing_a_display() {
    let (task, display) = start(&[]);
    assert!(pollster::block_on(task).is_err());
    assert!(display.try_recv().is_err());
}

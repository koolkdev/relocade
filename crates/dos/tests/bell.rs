#[path = "../../machine/tests/support/clock.rs"]
mod clock;
mod common;

use clock::ManualClock;
use relocade_dos::Dos;
use relocade_pc_speaker::SPEAKER_CONTROL_PORT;
use std::{
    future::{poll_fn, Future},
    pin::{pin, Pin},
    time::Duration,
};

fn drive_until(mut execution: Pin<&mut impl Future>, condition: impl Fn() -> bool) {
    for _ in 0..20 {
        common::assert_pending(execution.as_mut());
        if condition() {
            return;
        }
    }
    panic!("execution did not reach the expected bell state");
}

#[test]
fn bios_bells_allow_timer_irqs_and_video_changes_during_output() {
    pollster::block_on(async {
        let clock = ManualClock::default();
        let (runner, _host) = common::runner(clock.clone());
        let (ready, mut prepared) = futures_channel::oneshot::channel();
        let execution = runner.run(std::future::pending::<()>(), async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(
                &[
                    0xb8, 7, 0x0e, 0xcd, 0x10, // Teletype BEL.
                    0x9c, 0x58, 0xa3, 4, 3, // Save returned flags.
                    0xc6, 6, 0, 3, 1, // Mark the first bell complete.
                    0xb8, 0x0d, 0, 0xcd, 0x10, // Graphics mode before string output.
                    0xb8, 3, 0x13, 0xbb, 7, 2, // AH=13h, inline attributes, page 2.
                    0xb9, 3, 0, 0xba, 2, 1, 0xbd, 0x10, 3, // Three cells at row 1, column 2.
                    0xcd, 0x10, 0xb8, 0, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            dos.cpu().cpu_state().set_eflags(2); // Only BIOS enables IRQs during the wait.
            let memory = dos.cpu().memory();
            memory
                .view()
                .write(0x10310, &[b'A', 0x2e, 7, 0x5f, b'B', 0x1e])
                .unwrap();
            let io = dos.cpu().context().io.clone();
            ready.send((memory.clone(), io)).ok().unwrap();
            assert_eq!(dos.run(runner).await, Ok(0));
            assert_eq!(memory.view().read_u16(0x10304).unwrap() & 0x200, 0);
            assert_eq!(memory.view().read_u16(0x454).unwrap(), 0x0104);
        });
        let mut execution = pin!(execution);
        let (memory, io) = poll_fn(|cx| {
            assert!(execution.as_mut().poll(cx).is_pending());
            Pin::new(&mut prepared).poll(cx)
        })
        .await
        .unwrap();
        drive_until(execution.as_mut(), || {
            io.read_port(SPEAKER_CONTROL_PORT, 1) & 3 == 3
        });
        clock.advance(Duration::from_millis(55));
        drive_until(execution.as_mut(), || {
            memory.view().read_u32(0x46c).unwrap() == 1
        });
        assert_eq!(memory.view().read_u8(0x10300).unwrap(), 0);
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        clock.advance(Duration::from_millis(100));
        drive_until(execution.as_mut(), || {
            memory.view().read_u8(0x10300).unwrap() == 1
        });
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        assert_eq!(memory.view().read_u8(0x449).unwrap(), 0x0d);
        // A guest IRQ switches back to text while the string is suspended at BEL.
        // The suffix must use the new layout and the string's retained position.
        let timer = memory.view().read_u32(8 * 4).unwrap();
        let mut handler = vec![0x50, 0xb8, 3, 0, 0xcd, 0x10, 0x58, 0xea];
        handler.extend_from_slice(&timer.to_le_bytes());
        memory.view().write(0x600, &handler).unwrap();
        memory.view().write_u32(8 * 4, 0x600).unwrap();
        clock.advance(Duration::from_millis(155));
        drive_until(execution.as_mut(), || {
            memory.view().read_u8(0x449).unwrap() == 3
        });
        clock.advance(Duration::from_millis(199));
        common::assert_pending(execution.as_mut());
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 3);
        clock.advance(Duration::from_millis(200));
        assert_eq!(execution.await, Some(()));
        assert_eq!(io.read_port(SPEAKER_CONTROL_PORT, 1) & 3, 0);
        assert_eq!(
            memory.view().with_io(&io).read_u16(0xba0a6).unwrap(),
            0x1e42
        );
    });
}

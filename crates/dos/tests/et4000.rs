#![cfg(not(target_arch = "wasm32"))]
mod common;

use relocade_clock_test_support::ManualClock;
use relocade_dos::{Configuration, Dos, SoundCard, VideoCard};
use relocade_vga::FramebufferGeometry;
use relocade_x86::ExecutionExit;
use std::time::Duration;

#[test]
fn selected_et4000_runs_banked_graphics_cursor_and_text_through_dos() {
    pollster::block_on(async {
        let clock = ManualClock::default();
        let (runner, _host, machine) = common::runner(clock.clock()).await;
        runner
            .run(
                futures_timer::Delay::new(Duration::from_secs(30)),
                async |runner| {
                    let mut dos = Dos::with_configuration(
                        runner,
                        Configuration {
                            video_card: VideoCard::Et4000Ax,
                            sound_card: SoundCard::None,
                        },
                    )
                    .await
                    .unwrap();
                    let io = dos.cpu().context().io.clone();
                    assert_eq!(io.read_port(0x388, 1), 0xff);
                    assert_eq!(io.read_port(0x1ce, 2), 0xffff);
                    assert_eq!(io.read_port(0x1cf, 2), 0xffff);
                    let display = dos.video_card().framebuffer_access();
                    let frames = dos.frames();
                    let mut code = vec![
                        0x0e, 0x07, // PUSH CS; POP ES: query buffers in the COM segment.
                        0xbf, 0, 4, 0xb8, 0, 0x4f, 0xcd, 0x10, 0xa3, 0,
                        5, // Controller info; save AX.
                        0xa1, 0x12, 4, 0xa3, 0x0a,
                        5, // Save installed memory from the info block.
                        0xb9, 1, 1, 0xb8, 1, 0x4f, 0xcd, 0x10, 0xa3, 2, 5, // Query 101h.
                        0xbb, 1, 1, 0xb8, 2, 0x4f, 0xcd, 0x10, 0xa3, 4, 5, // Set 101h.
                        0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000h.
                        0x26, 0xc7, 0x06, 0xfe, 0xff, 0x34, 0x12, // Last word of bank zero.
                        0xbb, 0, 0, 0xba, 1, 0, 0xb8, 5, 0x4f, 0xcd,
                        0x10, // Write window -> bank one.
                        0x26, 0xc7, 0x06, 0, 0, 0x78, 0x56, // First word of bank one.
                        0xbb, 1, 0, 0xb8, 5, 0x4f, 0xcd, 0x10, // Read window -> bank one.
                        0x26, 0xa1, 0, 0, 0xa3, 6, 5, // Save the banked read.
                        0x31, 0xc0, 0xcd, 0x33, 0xa3, 8,
                        5, // Reset the resident mouse driver.
                        0xb9, 0, 1, 0xba, 102, 0, 0xb8, 4, 0, 0xcd,
                        0x33, // Position at the bank boundary.
                        0xb8, 1, 0, 0xcd, 0x33, 0xeb,
                        0, // Show cursor; a block boundary for inspection.
                    ];
                    let cursor_shown = 0x100 + code.len() as u32;
                    code.extend([0xb8, 2, 0, 0xcd, 0x33, 0xeb, 0]); // Hide cursor.
                    let cursor_hidden = 0x100 + code.len() as u32;
                    code.extend([
                        0xb8, 3, 0, 0xcd, 0x10, 0xb8, b'A', 0x0e, 0xcd, 0x10, 0xeb, 0,
                    ]);
                    let text_ready = 0x100 + code.len() as u32;
                    code.extend([0xeb, 0xfe]);
                    dos.launch_com(&code, Default::default()).unwrap();
                    let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
                    assert_eq!(
                        common::run_until(dos.cpu(), cursor_shown).await,
                        ExecutionExit::Interrupted
                    );
                    let memory = dos.cpu().memory();
                    for offset in [0x500, 0x502, 0x504] {
                        assert_eq!(memory.view().read_u16(base + offset).unwrap(), 0x004f);
                    }
                    assert_eq!(memory.view().read_u16(base + 0x50a).unwrap(), 16);
                    assert_eq!(memory.view().read_u16(base + 0x412).unwrap(), 640);
                    assert_eq!(memory.view().read_u16(base + 0x506).unwrap(), 0x5678);
                    assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 0xffff);
                    assert_eq!(io.read_port(0x3cd, 1), 0x11);
                    {
                        let mut display = display.borrow_mut();
                        let frame = display.framebuffer();
                        assert_eq!(
                            frame.geometry(),
                            Some(FramebufferGeometry::Graphics {
                                width: 640,
                                height: 480
                            })
                        );
                        assert_eq!(frame.pixel(254, 102).unwrap().value(), 0x34);
                        assert_eq!(
                            frame.pixel(256, 102).unwrap().value(),
                            0,
                            "cursor drawn through shared native access"
                        );
                    }
                    clock.advance_by(Duration::from_millis(20));
                    io.read_port(0x3da, 1);
                    assert_eq!(frames.recv().await.unwrap().geometry().size, [640, 480]);
                    assert_eq!(
                        common::run_until(dos.cpu(), cursor_hidden).await,
                        ExecutionExit::Interrupted
                    );
                    assert_eq!(
                        display
                            .borrow_mut()
                            .framebuffer()
                            .pixel(256, 102)
                            .unwrap()
                            .value(),
                        0x78
                    );
                    assert_eq!(
                        io.read_port(0x3cd, 1),
                        0x11,
                        "cursor access preserves CPU banks"
                    );
                    assert_eq!(
                        common::run_until(dos.cpu(), text_ready).await,
                        ExecutionExit::Interrupted
                    );
                    assert_eq!(
                        display
                            .borrow_mut()
                            .framebuffer()
                            .text_cell(0, 0)
                            .unwrap()
                            .value(),
                        0x0741
                    );
                },
            )
            .await
            .expect("ET4000 guest timed out");
        machine.shutdown().await.unwrap();
    });
}

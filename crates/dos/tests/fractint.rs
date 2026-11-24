//! Optional compatibility run using an externally supplied Fractint 19.6 package.
#![cfg(not(target_arch = "wasm32"))]
mod common;

use relocade_clock_test_support::ManualClock;
use relocade_dos::{
    Configuration, Dos, FileAccess, LoadOptions, ProcessStatus, SoundCard, VideoCard,
};
use relocade_vga::{FramebufferGeometry, Scanout};
use relocade_x86::{ExecutionExit, HostExit, Segment};
use std::{path::Path, time::Duration};

async fn render(directory: &Path, native: bool) -> Vec<u8> {
    let clock = ManualClock::default();
    let (runner, _host, machine) = common::runner(clock.clock()).await;
    let pixels = runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(120)),
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
                for entry in std::fs::read_dir(directory).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_file() {
                        dos.mount_file(
                            entry.file_name().to_str().unwrap(),
                            std::fs::read(entry.path()).unwrap(),
                            FileAccess::ReadOnly,
                        )
                        .unwrap();
                    }
                }
                // SF5 is Fractint's 640x480x256 autodetected driver. Disabling VESA
                // detection exercises its actual Tseng probes and native bank routine.
                let command = if native {
                    b" video=sf5 vesadetect=no batch=yes maxiter=8 savename=RESULT.gif".as_slice()
                } else {
                    b" video=sf5 batch=yes maxiter=8 savename=RESULT.gif".as_slice()
                };
                dos.launch_mz(
                    &std::fs::read(directory.join("FRACTINT.EXE")).unwrap(),
                    LoadOptions {
                        command_tail: command,
                        program_path: b"C:\\FRACTINT.EXE",
                        environment: &[b"PATH=C:\\", b"TEMP=C:\\"],
                    },
                )
                .unwrap();
                let frames = dos.frames();
                let display = dos.video_card().framebuffer_access();
                let mut last_image = None;
                for block in 0..2_000_000 {
                    let exit = common::checkpoint(dos.cpu()).await;
                    while let Ok(frame) = frames.try_recv() {
                        if matches!(frame.scanout(), Scanout::Packed(_)) {
                            last_image = Some(frame);
                        }
                    }
                    if exit != ExecutionExit::Interrupted {
                        if exit != ExecutionExit::Host(HostExit::Return) {
                            let cpu = dos.cpu().cpu_state();
                            let cs = cpu.segment(Segment::Cs);
                            let mut code = [0; 16];
                            let code = dos
                                .cpu()
                                .memory()
                                .view()
                                .read(cs.base + cpu.eip(), &mut code)
                                .map(|()| code);
                            panic!(
                                "{exit:?} at {:04x}:{:04x}, bytes {code:02x?}",
                                cs.selector,
                                cpu.eip()
                            );
                        }
                        assert_eq!(
                            dos.process().unwrap().status(),
                            ProcessStatus::Exited(Ok(0)),
                            "AX={:#x} BX={:#x}",
                            dos.cpu().cpu_state().ax(),
                            dos.cpu().cpu_state().bx()
                        );
                        let gif = dos
                            .read_file("RESULT.GIF")
                            .expect("Fractint saved the rendered image");
                        assert!(gif.starts_with(b"GIF89a") || gif.starts_with(b"GIF87a"));
                        assert_eq!(&gif[6..10], &[0x80, 2, 0xe0, 1]);
                        assert_eq!(
                            display.borrow_mut().framebuffer().geometry(),
                            Some(FramebufferGeometry::Text {
                                columns: 80,
                                rows: 25
                            })
                        );
                        let frame = last_image.expect("captured extended graphics mode");
                        let Scanout::Packed(layout) = *frame.scanout() else {
                            unreachable!()
                        };
                        assert_eq!(layout.size(), [640, 480]);
                        let pixels: Vec<_> = (0..480)
                            .flat_map(|y| (0..640).map(move |x| (x, y)))
                            .map(|(x, y)| frame.vram_byte(layout.pixel_address(x, y)).unwrap())
                            .collect();
                        assert!(
                            pixels.iter().any(|&pixel| pixel != pixels[0]),
                            "nonuniform fractal image"
                        );
                        return pixels;
                    }
                    if block % 256 == 0 {
                        clock.advance_by(Duration::from_millis(1));
                        futures_timer::Delay::new(Duration::ZERO).await;
                    }
                }
                panic!("Fractint did not finish its batch render");
            },
        )
        .await
        .expect("Fractint compatibility run timed out");
    machine.shutdown().await.unwrap();
    pixels
}

#[test]
#[ignore = "requires the external Fractint 19.6 package; set FRACTINT196_DIR"]
fn native_tseng_and_banked_vesa_render_the_same_image_and_return_to_text() {
    let directory = std::env::var_os("FRACTINT196_DIR").expect("set FRACTINT196_DIR");
    pollster::block_on(async {
        let native = render(Path::new(&directory), true).await;
        let vesa = render(Path::new(&directory), false).await;
        assert_eq!(native, vesa);
    });
}

//! Guest BIOS calls, MMIO and DAC writes feed the ordinary captured GPU path.
#![cfg(not(target_arch = "wasm32"))]

mod common;
#[path = "../../vga/tests/gpu/readback.rs"]
mod readback;

use common::vga_support::fixture;
use relocade_dos::Dos;
use relocade_vga::{CapturedFrame, VgaRenderer};
use relocade_x86::{ExecutionExit, HostExit};

async fn run(dos: &mut Dos, program: &[u8]) -> CapturedFrame {
    dos.launch_com(program, Default::default()).unwrap();
    assert_eq!(
        dos.cpu().run(100_000).await,
        ExecutionExit::Host(HostExit::Return)
    );
    fixture::capture(&dos.vga().borrow())
}

async fn render(
    renderer: &mut VgaRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    capture: &CapturedFrame,
    size: [u32; 2],
) -> Vec<u8> {
    renderer.upload(queue, capture);
    let mut encoder = device.create_command_encoder(&Default::default());
    let image = renderer.encode(&mut encoder).unwrap().image;
    queue.submit([encoder.finish()]);
    assert_eq!(image.size(), size);
    readback::read_pixels(device, queue, image.texture()).await
}

#[test]
fn guest_mode_13h_pixels_palette_and_text_return_render_from_retained_captures() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let mut renderer = VgaRenderer::new(&device).unwrap();
        let mut dos = common::dos().await;
        let mut program = vec![
            0xb8, 0x13, 0, 0xcd, 0x10, // INT 10h: mode 13h.
            0xb8, 0, 0xa0, 0x8e, 0xc0, // ES = A000.
            0x31, 0xff, // DI = 0.
            0xb9, 0, 0xfa, // CX = 64000.
            0xb0, 9, 0xfc, 0xf3, 0xaa, // CLD; REP STOSB: bright blue pixels.
            0xba, 0xc8, 3, 0xb0, 6, 0xee, // DAC index 6.
            0x42, 0xb0, 63, 0xee, // Red = 63.
            0x30, 0xc0, 0xee, 0xee, // Green/blue = 0.
        ];
        let offsets: [u16; 8] = [0, 319, 320, 16383, 16384, 32768, 49152, 63999];
        for offset in offsets {
            let [low, high] = offset.to_le_bytes();
            program.extend_from_slice(&[0x26, 0xc6, 0x06, low, high, 6]); // ES:[offset] = 6.
        }
        program.push(0xc3); // RET exits the COM program through its PSP.
        let graphics = run(&mut dos, &program).await;
        assert_eq!(graphics.display.geometry().pixel_aspect_ratio(), (5, 6));
        let pixels = render(&mut renderer, &device, &queue, &graphics, [320, 200]).await;
        for (offset, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let expected = if offsets.contains(&(offset as u16)) {
                [255, 0, 0, 255]
            } else {
                [85, 85, 255, 255]
            };
            assert_eq!(*pixel, expected, "guest pixel {offset}");
        }

        // A second guest changes the DAC; console output stays in the byte
        // stream until graphics-mode glyph drawing is supported. VRAM is shared.
        let recolored = run(
            &mut dos,
            &[
                0xba, 0xc8, 3, 0xb0, 6, 0xee, 0x42, 0x30, 0xc0, 0xee, // Red = 0.
                0xb0, 63, 0xee, // Green = 63.
                0x30, 0xc0, 0xee, // Blue = 0.
                0xb4, 2, 0xb2, b'A', 0xcd, 0x21, // DOS console output.
                0xc3, // RET.
            ],
        )
        .await;
        assert_eq!(common::output(dos.stdout()), b"A");
        for plane in 0..4 {
            assert!(std::ptr::eq(
                graphics.display.plane(plane),
                recolored.display.plane(plane)
            ));
        }
        let green = render(&mut renderer, &device, &queue, &recolored, [320, 200]).await;
        assert_eq!(&green[..4], &[0, 255, 0, 255]);
        assert_eq!(&green[4..8], &[85, 85, 255, 255]);

        let text = run(
            &mut dos,
            &[
                0xb8, 3, 0, 0xcd, 0x10, // Return to mode 3; restore the font.
                0xb8, 0, 1, 0xb9, 7, 0x20, 0xcd, 0x10, // Hide cursor.
                0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
                0x26, 0xc7, 0x06, 0, 0, 0xdb, 4, // First cell is a red full block.
                0xc3,
            ],
        )
        .await;
        drop(dos);
        let restored = render(&mut renderer, &device, &queue, &text, [640, 200]).await;
        for (offset, pixel) in restored.as_chunks::<4>().0.iter().enumerate() {
            let expected = if offset % 640 < 8 && offset / 640 < 8 {
                [170, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(*pixel, expected, "restored text pixel {offset}");
        }
        let retained = render(&mut renderer, &device, &queue, &graphics, [320, 200]).await;
        assert_eq!(retained, pixels);
    });
}

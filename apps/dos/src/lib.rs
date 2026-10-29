//! One interactive DOS viewport hosted by the reusable winit application harness.
#![forbid(unsafe_code)]

mod graphics;
mod keyboard;
pub use graphics::Graphics;

use futures_util::StreamExt;
use relocade_app::{Application, Context, Input, WindowApplication};
use relocade_dos::Dos;
use relocade_machine::Runner;
use relocade_vga::FrameSource;
use std::{
    future::{pending, poll_fn, Future},
    io,
    pin::pin,
};

/// A COM or MZ image. DOS, CPU and mutable VGA state are created on execution.
pub struct DosApplication {
    program: Vec<u8>,
}
impl DosApplication {
    pub fn new(program: Vec<u8>) -> Self {
        Self { program }
    }
}
impl Application for DosApplication {
    type Input = Input;
    type Source = FrameSource;
    type Output = ();

    async fn run(self, mut context: Context<Input, FrameSource>) -> Result<(), String> {
        Runner::with_clock(context.clock.clone())
            .run(async |runner| {
                let mut dos = Dos::new(runner).await.map_err(|error| error.to_string())?;
                if self.program.starts_with(b"MZ") {
                    dos.launch_mz(&self.program, Default::default())
                } else {
                    dos.launch_com(&self.program, Default::default())
                }
                .map_err(|error| error.to_string())?;
                context.publish(dos.frame_source());
                let mut keyboard = keyboard::KeyboardInput::new(dos.keyboard());
                let input = async {
                    while let Some(event) = context.input.next().await {
                        keyboard.handle(event);
                    }
                    pending::<Result<(), String>>().await
                };
                let execution = async {
                    let (mut stdout, mut stderr) = (dos.stdout(), dos.stderr());
                    {
                        let mut run = pin!(dos.run(runner));
                        poll_fn(|cx| {
                            let result = run.as_mut().poll(cx);
                            // Console output already updates VGA. Drain its host byte
                            // buffers after each poll instead of retaining an unused log.
                            io::copy(&mut stdout, &mut io::sink()).expect("DOS buffer read");
                            io::copy(&mut stderr, &mut io::sink()).expect("DOS buffer read");
                            result
                        })
                        .await
                        .map_err(|error| error.to_string())?;
                    }
                    // Program exit leaves DOS and VGA alive, including cursor/blink
                    // captures. Closing the host cancels this session on its owner.
                    pending::<Result<(), String>>().await
                };
                futures_lite::future::or(execution, input).await
            })
            .await
    }
}
impl WindowApplication for DosApplication {
    type Graphics = Graphics;

    fn create_graphics(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Graphics, String> {
        Graphics::new(device, queue, format)
    }
    fn draw(
        graphics: &mut Graphics,
        source: &FrameSource,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String> {
        graphics.draw(source, attachment, size);
        Ok(())
    }
}

/// A tiny COM example: echo keys through DOS until Escape, then leave its output.
pub fn demo_program() -> Vec<u8> {
    let mut program = vec![
        0xba, 0x17, 0x01, // mov dx,message
        0xb4, 0x09, 0xcd, 0x21, // DOS print string
        0xb4, 0x01, 0xcd, 0x21, // read and echo a key
        0x3c, 0x1b, 0x74, 0x02, // Escape exits
        0xeb, 0xf6, // otherwise read another key
        0xb8, 0x00, 0x4c, 0xcd, 0x21, // DOS exit
        0x90,
    ];
    program.extend_from_slice(b"Relocade DOS\r\nType to echo; Escape ends the program.\r\n$");
    program
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn open(program: &[u8]) -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    relocade_app::open(DosApplication::new(program.to_vec()), Default::default())
        .map(|control| relocade_app::web::control(control, |()| wasm_bindgen::JsValue::UNDEFINED))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn demo() -> Vec<u8> {
    demo_program()
}

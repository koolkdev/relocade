//! DOS execution connected to the shared VGA renderer and application host.
#![deny(unsafe_code)]

mod execution;

use relocade_app::{Application, Context, Control, Exit, OnCompletion, WindowOptions};
use relocade_vga::{Display, VgaRenderer};

/// Fixed program used by the basic native/browser visualization entrypoints.
/// Fills VGA text memory with blue cells, writes "Relocade", and exits.
pub const DEMO_PROGRAM: &[u8] = &[
    0xb8, 0, 0xb8, 0x8e, 0xc0, // ES = B800.
    0x31, 0xff, 0xfc, // DI = 0; CLD.
    0xb8, b' ', 0x1f, 0xb9, 0xd0, 7, 0xf3, 0xab, // Fill 2,000 cells.
    0xbf, 0xc8, 7, // Row 12, column 36.
    0xb8, b'R', 0x1f, 0xab, 0xb0, b'e', 0xab, 0xb0, b'l', 0xab, 0xb0, b'o', 0xab, 0xb0, b'c', 0xab,
    0xb0, b'a', 0xab, 0xb0, b'd', 0xab, 0xb0, b'e', 0xab, 0xb8, 0, 0x4c, 0xcd,
    0x21, // Exit 0; the host retains the display.
];

/// A COM program to execute with VGA visualization.
/// DOS state is constructed on the application worker from these supplied bytes.
pub struct DosApplication {
    program: Vec<u8>,
}

impl DosApplication {
    pub fn new(program: Vec<u8>) -> Self {
        Self { program }
    }

    /// Opens a native window or browser canvas and starts execution.
    /// The final display remains visible until the host closes.
    pub fn open(self) -> Result<Control<u8>, String> {
        relocade_app::run::<VgaRenderer, _>(
            self,
            WindowOptions {
                title: "Relocade DOS".into(),
                size: [960, 720],
                on_completion: OnCompletion::KeepOpen,
            },
        )
    }
}

impl Application for DosApplication {
    type Update = Display;
    type Output = u8;

    async fn run(self, context: Context<Display>) -> Result<Exit<u8>, String> {
        self.execute(context).await
    }
}

/// Starts the same application from browser-supplied program bytes.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn open(program: Vec<u8>) -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    relocade_app::web::launch(DosApplication::new(program).open(), |exit| {
        match exit {
            Exit::Completed(code) => code,
            Exit::Cancelled => 0,
        }
        .into()
    })
}

/// Starts the fixed visualization demonstration after browser initialization.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start() -> Result<relocade_app::web::BrowserControl, wasm_bindgen::JsValue> {
    open(DEMO_PROGRAM.to_vec())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

//! Rust DOS command console, sharing the guest's keyboard and VGA state.
#![forbid(unsafe_code)]

use futures_channel::oneshot;
use futures_lite::future::or;
use std::future::pending;

mod console;
pub use console::run_console;

/// Opens the Rust DOS console using the shared application host.
/// Native execution returns after cleanup; browser startup returns immediately.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn run() -> Result<(), String> {
    relocade_app::run_app("Relocade DOS", |mut app| async move {
        let (endpoints, ready) = oneshot::channel();
        let mut app_result = Ok(());
        let execution = relocade_machine::ExecutionWorker::run_until(
            async {
                let startup = or(
                    async {
                        app_result = app.closing().await;
                        None
                    },
                    async { Some(ready.await) },
                )
                .await;
                match startup {
                    None => {}
                    // Execution reports its startup error through its completion.
                    Some(Err(_)) => pending().await,
                    Some(Ok((keyboard, frames))) => {
                        app_result = async {
                            app.attach_keyboard(keyboard)?;
                            app.attach_display(move |device, _| {
                                relocade_vga::VgaDisplay::new(device, frames)
                                    .map_err(|error| error.to_string())
                            })?;
                            app.closing().await
                        }
                        .await;
                    }
                }
            },
            async move |runner| {
                let mut dos = relocade_dos::Dos::new(runner)
                    .await
                    .map_err(|error| error.to_string())?;
                if endpoints
                    .send((dos.keyboard_input(), dos.frame_source()))
                    .is_err()
                {
                    return Ok(());
                }
                run_console(&mut dos).await;
                Ok(())
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        execution.unwrap_or(app_result)
    })
}

//! Native Rust command console, sharing the PC's keyboard layout and VGA state.
#![forbid(unsafe_code)]

use futures_channel::oneshot;
use futures_lite::future::or;

mod console;
pub use console::run_console;

/// Opens the Rust DOS console using the shared application host.
/// Native execution returns after cleanup; browser startup returns immediately.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn run() -> Result<(), String> {
    relocade_app::run_app("Relocade DOS", |mut app| async move {
        let output = app.open_audio().ok();
        let (endpoints, ready) = oneshot::channel();
        let closing = app.closing();
        let mut app_result = Ok(());
        let execution = relocade_machine::ExecutionWorker::run_until(
            async {
                app_result = or(closing, async {
                    let (keyboard, frames) = ready
                        .await
                        .map_err(|_| "Execution ended during startup".to_string())??;
                    app.attach_keyboard(keyboard)?;
                    app.attach_display(move |device, _| {
                        relocade_vga::VgaDisplay::new(device, frames)
                            .map_err(|error| error.to_string())
                    })?;
                    app.closing().await
                })
                .await;
            },
            async move |runner| {
                let startup = async {
                    let dos = relocade_dos::Dos::new(runner)
                        .await
                        .map_err(|error| error.to_string())?;
                    if let Some((config, samples)) = output {
                        dos.speaker().borrow_mut().attach_audio(config, samples)?;
                    }
                    Ok::<_, String>(dos)
                }
                .await;
                let (keyboard, events) = relocade_frontend::keyboard::channel();
                let _ = endpoints.send(
                    startup
                        .as_ref()
                        .map(|dos| (keyboard, dos.frame_source()))
                        .map_err(Clone::clone),
                );
                let mut dos = startup?;
                run_console(dos.native_console(events)).await;
                Ok(())
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        execution.unwrap_or(app_result)
    })
}

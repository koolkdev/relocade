//! Rust DOS command console, sharing the guest's keyboard and VGA state.
#![forbid(unsafe_code)]

mod console;
pub use console::run_console;

/// Opens the Rust DOS console using the shared application host.
/// Native execution returns after cleanup; browser startup returns immediately.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn run() -> Result<(), String> {
    relocade_app::run_app("Relocade DOS", |mut app| async move {
        let mut peripherals = app.peripherals();
        relocade_machine::ExecutionWorker::run_until(app.closing(), async move |runner| {
            let mut dos = relocade_dos::Dos::new(runner)
                .await
                .map_err(|error| error.to_string())?;
            peripherals.attach_keyboard(dos.keyboard_input())?;
            let source = dos.frame_source();
            peripherals.attach_display(move |device, _| {
                relocade_vga::VgaDisplay::new(device, source).map_err(|error| error.to_string())
            })?;
            run_console(&mut dos).await;
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or(Ok(()))
    })
}

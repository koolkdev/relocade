//! Native Rust command console, sharing the PC's keyboard layout and VGA state.
#![forbid(unsafe_code)]

use futures_channel::oneshot;
use futures_lite::future::{or, zip};
use futures_util::FutureExt;

mod console;
pub use console::run_console;

/// Opens the Rust DOS console using the shared application host.
/// Native execution returns after cleanup; browser startup returns immediately.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn run() -> Result<(), String> {
    run_with_configuration(relocade_dos::Configuration::default())
}

/// Opens the console with the selected PC hardware.
/// This Rust entrypoint shares the lifecycle of [`run`], including host audio.
pub fn run_with_configuration(configuration: relocade_dos::Configuration) -> Result<(), String> {
    relocade_app::run_app("Relocade DOS", move |app| async move {
        let compiler =
            relocade_machine::CompilerService::start().map_err(|error| error.to_string())?;
        let result = run_machine(app, &compiler, configuration).await;
        compiler
            .shutdown()
            .await
            .map_err(|error| error.to_string())?;
        result
    })
}

async fn run_machine(
    mut app: relocade_app::AppHost,
    compiler: &relocade_machine::CompilerService,
    configuration: relocade_dos::Configuration,
) -> Result<(), String> {
    let output = app.open_audio().ok();
    let (endpoints, ready) = oneshot::channel();
    let closing = app.closing().shared();
    let mut machine = relocade_machine::Machine::new(relocade_dos::configuration::memory())
        .map_err(|error| error.to_string())?;
    let cpu = match machine.create_cpu(relocade_dos::configuration::cpu(), Some(compiler)) {
        Ok(cpu) => cpu,
        Err(error) => {
            machine
                .shutdown()
                .await
                .map_err(|error| error.to_string())?;
            return Err(error.to_string());
        }
    };
    let execution = cpu.run_until(closing.clone(), async move |runner| {
        let mut dos = relocade_dos::Dos::with_configuration(runner, configuration)
            .await
            .map_err(|error| error.to_string())?;
        if let Some((config, samples)) = output {
            dos.attach_audio(config, samples).await?;
        }
        let (keyboard, events) = relocade_frontend::keyboard::channel();
        let _ = endpoints.send((keyboard, dos.mouse_input(), dos.frames()));
        run_console(dos.native_console(events)).await;
        Ok::<(), String>(())
    });
    let frontend = async {
        let attach = async {
            if let Ok((keyboard, mouse, frames)) = ready.await {
                app.attach_keyboard(keyboard)?;
                app.attach_mouse(mouse)?;
                app.attach_display(move |device, _| {
                    relocade_vga::VgaDisplay::new(device, frames).map_err(|error| error.to_string())
                })?;
            }
            Ok(())
        };
        let result = or(closing, attach).await;
        if result.is_err() {
            app.request_close();
        }
        result
    };
    let (execution, frontend) = zip(execution, frontend).await;
    let shutdown = machine.shutdown().await;
    let execution = execution.map_err(|error| error.to_string())?;
    shutdown.map_err(|error| error.to_string())?;
    if let Some(result) = execution {
        result?;
    }
    frontend
}

//! Native Rust command console, sharing the PC's keyboard layout and VGA state.
#![forbid(unsafe_code)]

use futures_channel::oneshot;
use futures_lite::future::or;

mod console;
mod printing;
mod session;
pub use console::run_console;

/// The console app's default PC, with its printer on the first parallel port.
pub fn default_configuration() -> relocade_dos::Configuration {
    relocade_dos::Configuration {
        parallel_port: Some(relocade_dos::ParallelPort {
            base: 0x378,
            irq: 5,
        }),
        ..Default::default()
    }
}

/// Opens the Rust DOS console using the shared application host.
/// Native execution returns after cleanup; browser startup returns immediately.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn run() -> Result<(), String> {
    run_with_configuration(default_configuration())
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
    let (saves, printouts) = async_channel::bounded::<printing::SavePrintout>(1);
    let closing = app.closing();
    let mut app_result = Ok(());
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
    let execution = cpu.run_until(
        async {
            app_result = or(closing, async {
                let (keyboard, mouse, frames) = ready
                    .await
                    .map_err(|_| "Execution ended during startup".to_string())??;
                app.attach_keyboard(keyboard)?;
                app.attach_mouse(mouse)?;
                app.attach_display(move |device, _| {
                    relocade_vga::VgaDisplay::new(device, frames).map_err(|error| error.to_string())
                })?;
                while let Ok(printout) = printouts.recv().await {
                    printout.save().await;
                }
                app.closing().await
            })
            .await;
        },
        async move |runner| {
            let startup = async {
                let (mut dos, printer) = session::start(runner, configuration)
                    .await
                    .map_err(|error| error.to_string())?;
                if let Some((config, samples)) = output {
                    dos.attach_audio(config, samples).await?;
                }
                Ok::<_, String>((dos, printer))
            }
            .await;
            let (keyboard, events) = relocade_frontend::keyboard::channel();
            let _ = endpoints.send(
                startup
                    .as_ref()
                    .map(|(dos, _)| (keyboard, dos.mouse_input(), dos.frames()))
                    .map_err(Clone::clone),
            );
            let (mut dos, printer) = startup?;
            let printout = printer.map(|capture| printing::Printout { capture, saves });
            console::Console::new(dos.native_console(events), printout)
                .run()
                .await;
            Ok(())
        },
    );
    let execution = execution.await;
    let shutdown = machine.shutdown().await;
    let execution = execution.map_err(|error| error.to_string())?;
    shutdown.map_err(|error| error.to_string())?;
    execution.unwrap_or(app_result)
}

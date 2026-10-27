#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    use relocade_app::Exit;
    use relocade_dos_app::{DosApplication, DEMO_PROGRAM};
    match DosApplication::new(DEMO_PROGRAM.to_vec())
        .open()
        .and_then(pollster::block_on)
    {
        Ok(Exit::Completed(code)) => code.into(),
        Ok(Exit::Cancelled) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

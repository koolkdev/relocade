#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: relocade-dos-app PROGRAM.COM|PROGRAM.EXE")?;
    relocade_dos_app::run(std::fs::read(path)?)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}

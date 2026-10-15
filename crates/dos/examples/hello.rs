#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};
    use wasm86_dos::{load_com, DosStop};
    use wasm86_runtime::Runtime;
    use wasm86_runtime_wasmtime::WasmtimeBackend;
    use wasm86_x86::ExecutionExit;

    pollster::block_on(async {
        let image = [
            0xba, 0x0c, 1, 0xb4, 9, 0xcd, 0x21, 0xb8, 0, 0x4c, 0xcd, 0x21, b'H', b'e', b'l', b'l',
            b'o', b' ', b'D', b'O', b'S', b'!', b'\r', b'\n', b'$',
        ];
        let runtime = Runtime::new(WasmtimeBackend::new()?);
        let mut session = runtime.attach(load_com(&image, &[])?).await?;
        loop {
            let outcome = session.run(10000)?;
            let output = session.with_environment_mut(|dos| dos.take_output());
            io::stdout().write_all(&output)?;
            match outcome {
                ExecutionExit::BudgetExhausted => continue,
                ExecutionExit::HostYield { .. } => {
                    match session.with_environment(|dos| dos.stop()) {
                        Some(DosStop::Exited(0)) => return Ok(()),
                        reason => return Err(format!("DOS stopped: {reason:?}").into()),
                    }
                }
                reason => return Err(format!("guest stopped: {reason:?}").into()),
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // This console example uses native stdout; browser execution is covered by tests/com.rs.
}

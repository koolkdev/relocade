use relocade_dos::Dos;

/// Runs the Rust console on the DOS execution owner. Executable lookup is left
/// to future application policy; supplied images can use DOS's launch APIs.
pub async fn run_console(dos: &mut Dos) {
    loop {
        let console = dos.console();
        console.write(b"> ");
        let line = console.read_line().await;
        let line = line.trim_ascii_start();
        let end = line
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(line.len());
        let (command, arguments) = line.split_at(end);
        if command.eq_ignore_ascii_case(b"echo") {
            console.write(arguments.trim_ascii_start());
            console.write(b"\r\n");
        } else if !command.is_empty() {
            console.write(b"Unknown command\r\n");
        }
        futures_lite::future::yield_now().await;
    }
}

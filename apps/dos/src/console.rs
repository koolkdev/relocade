use relocade_dos::{devices::AnsiConsole, Dos, DosError};

mod banner;

/// Installs ANSI CON and runs the Rust console on the DOS execution owner.
/// Executable lookup is left to future application policy; supplied images can
/// use DOS's launch APIs.
pub async fn run_console(dos: &mut Dos) -> Result<(), DosError> {
    dos.install_driver(AnsiConsole::new(dos));
    dos.console().write(&banner::bytes()).await?;
    loop {
        let console = dos.console();
        console.write(b"   \x1b[0;1;32m>\x1b[0m ").await?;
        let line = console.read_line().await?;
        let line = line.trim_ascii_start();
        let end = line
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(line.len());
        let (command, arguments) = line.split_at(end);
        if command.eq_ignore_ascii_case(b"echo") {
            console.write(arguments.trim_ascii_start()).await?;
            console.write(b"\r\n").await?;
        } else if !command.is_empty() {
            console.write(b"Unknown command\r\n").await?;
        }
        futures_lite::future::yield_now().await;
    }
}

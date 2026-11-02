use relocade_dos::{Dos, DosError};

const BANNER: &str = r"  ____       _                     _
 |  _ \ ___ | | ___   ___ __ _  __| | ___
 | |_) / _ \| |/ _ \ / __/ _` |/ _` |/ _ \
 |  _ <  __/| | (_) | (_| (_| | (_| |  __/
 |_| \_\___||_|\___/ \___\__,_|\__,_|\___|

";

/// Runs the Rust console on the DOS execution owner. Executable lookup is left
/// to future application policy; supplied images can use DOS's launch APIs.
pub async fn run_console(dos: &mut Dos) -> Result<(), DosError> {
    for line in BANNER.lines() {
        dos.console().write(line.as_bytes()).await?;
        dos.console().write(b"\r\n").await?;
    }
    loop {
        let console = dos.console();
        console.write(b"> ").await?;
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

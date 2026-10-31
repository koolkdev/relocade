use relocade_dos::Dos;

const BANNER: &str = r"  ____       _                     _
 |  _ \ ___ | | ___   ___ __ _  __| | ___
 | |_) / _ \| |/ _ \ / __/ _` |/ _` |/ _ \
 |  _ <  __/| | (_) | (_| (_| | (_| |  __/
 |_| \_\___||_|\___/ \___\__,_|\__,_|\___|

  Rust DOS console
  Type ECHO <text> to print a line.

";

/// Runs the Rust console on the DOS execution owner. Executable lookup is left
/// to future application policy; supplied images can use DOS's launch APIs.
pub async fn run_console(dos: &mut Dos) {
    for line in BANNER.lines() {
        dos.console().write(line.as_bytes());
        dos.console().write(b"\r\n");
    }
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

use relocade_dos::Dos;
use relocade_frontend::keyboard::KeyboardEvents;

const BANNER: &str = r"  ____       _                     _
 |  _ \ ___ | | ___   ___ __ _  __| | ___
 | |_) / _ \| |/ _ \ / __/ _` |/ _` |/ _ \
 |  _ <  __/| | (_) | (_| (_| | (_| |  __/
 |_| \_\___||_|\___/ \___\__,_|\__,_|\___|

";

/// Runs the Rust prompt, driving PC hardware while it waits for keyboard input.
pub async fn run_console(dos: &mut Dos, events: KeyboardEvents) {
    let mut console = dos.native_console(events);
    for line in BANNER.lines() {
        console.write(line.as_bytes());
        console.write(b"\r\n");
    }
    loop {
        console.write(b"> ");
        let Some(line) = dos.wait_for(console.read_line()).await else {
            return;
        };
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
    }
}

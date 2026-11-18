use crate::printing::Printout;
use relocade_dos::NativeConsole;

const BANNER: &str = r"  ____       _                     _
 |  _ \ ___ | | ___   ___ __ _  __| | ___
 | |_) / _ \| |/ _ \ / __/ _` |/ _` |/ _ \
 |  _ <  __/| | (_) | (_| (_| | (_| |  __/
 |_| \_\___||_|\___/ \___\__,_|\__,_|\___|

";

/// Runs the standalone Rust command prompt on the DOS execution owner.
pub async fn run_console(console: NativeConsole<'_>) {
    Console::new(console, None).run().await;
}

pub(crate) struct Console<'a> {
    console: NativeConsole<'a>,
    printer: Option<Printout>,
}

impl<'a> Console<'a> {
    pub fn new(console: NativeConsole<'a>, printer: Option<Printout>) -> Self {
        Self { console, printer }
    }

    pub async fn run(mut self) {
        for line in BANNER.lines() {
            self.console.write(line.as_bytes());
            self.console.write(b"\r\n");
        }
        loop {
            self.console.write(b"> ");
            let Some(line) = self.console.read_line().await else {
                return;
            };
            if let Err(error) = self.command(&line).await {
                self.console.write(error.as_bytes());
                self.console.write(b"\r\n");
            }
        }
    }

    async fn command(&self, line: &[u8]) -> Result<(), String> {
        let line = line.trim_ascii_start();
        let end = line
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(line.len());
        let (command, arguments) = line.split_at(end);
        let arguments = arguments.trim_ascii_start();
        if command.eq_ignore_ascii_case(b"echo") {
            self.console.write(arguments);
            self.console.write(b"\r\n");
        } else if command.eq_ignore_ascii_case(b"printout") {
            let printout = self.printer.as_ref().ok_or("No printer is attached")?;
            if arguments.trim_ascii_end().eq_ignore_ascii_case(b"save") {
                printout.save_pdf().await?;
            } else if arguments.trim_ascii_end().eq_ignore_ascii_case(b"clear") {
                printout
                    .capture
                    .clear()
                    .await
                    .map_err(|error| error.to_string())?;
                self.console.write(b"Printer output cleared\r\n");
            } else {
                return Err("Usage: PRINTOUT SAVE or PRINTOUT CLEAR".into());
            }
        } else if command.eq_ignore_ascii_case(b"help") {
            self.console.write(b"ECHO text             Display text\r\nPRINTOUT SAVE         Save captured output as a PDF\r\nPRINTOUT CLEAR        Discard captured output\r\n");
        } else if !command.is_empty() {
            self.console.write(b"Unknown command\r\n");
        }
        Ok(())
    }
}

mod command_line;

use crate::printing::Printout;
use command_line::CommandLine;
use relocade_dos::{ConsoleFile, DosError, NativeConsole};

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

    async fn command(&self, bytes: &[u8]) -> Result<(), String> {
        let Some(line) = CommandLine::parse(bytes)? else {
            return Ok(());
        };
        let destination = line
            .stdout
            .as_ref()
            .map(|redirect| {
                self.console
                    .open_output(&redirect.path, redirect.mode)
                    .map_err(|error| file_error(&redirect.path, error))
            })
            .transpose()?;

        let output = self.execute(&line).await?;
        if let Some(destination) = destination {
            if !output.is_empty() {
                self.write_output(&destination, &output).await?;
            }
        } else {
            self.console.write(&output);
        }
        Ok(())
    }

    async fn execute(&self, line: &CommandLine<'_>) -> Result<Vec<u8>, String> {
        if line.name.eq_ignore_ascii_case(b"echo") {
            let mut text = line.argument_text();
            text.extend_from_slice(b"\r\n");
            Ok(text)
        } else if line.name.eq_ignore_ascii_case(b"printout") {
            let printout = self.printer.as_ref().ok_or("No printer is attached")?;
            match line.arguments.as_slice() {
                [argument] if argument.value.eq_ignore_ascii_case(b"save") => {
                    printout.save_pdf().await?;
                    Ok(Vec::new())
                }
                [argument] if argument.value.eq_ignore_ascii_case(b"clear") => {
                    printout
                        .capture
                        .clear()
                        .await
                        .map_err(|error| error.to_string())?;
                    Ok(b"Printer output cleared\r\n".to_vec())
                }
                _ => Err("Usage: PRINTOUT SAVE or PRINTOUT CLEAR".into()),
            }
        } else if line.name.eq_ignore_ascii_case(b"help") {
            Ok(b"ECHO text      Display text\r\nPRINTOUT SAVE  Save captured output as a PDF\r\nPRINTOUT CLEAR  Discard captured output\r\n> or >> path    Write or append to a DOS file or device\r\n".to_vec())
        } else {
            Err("Unknown command".into())
        }
    }

    async fn write_output(&self, destination: &ConsoleFile, output: &[u8]) -> Result<(), String> {
        // The app wires both names to this capture. The exclusive prompt borrow
        // prevents a guest write between the capacity check and the line write.
        if let Some(job) = self.printer.as_ref().filter(|_| {
            destination
                .device()
                .is_some_and(|device| matches!(&device.name(), b"PRN     " | b"LPT1    "))
        }) {
            let available = job
                .capture
                .remaining_capacity()
                .await
                .map_err(|error| error.to_string())?;
            if output.len() > available {
                return Err(
                    "Printer capture is full. Use PRINTOUT CLEAR to start a new job".into(),
                );
            }
        }
        destination
            .write_all(output)
            .await
            .map_err(|error| error.to_string())
    }
}

fn file_error(path: &[u8], error: DosError) -> String {
    let reason = match error {
        DosError::PathNotFound => "Path not found",
        DosError::FileNotFound => "File not found",
        DosError::AccessDenied => "Access denied",
        _ => return format!("{} ({error})", String::from_utf8_lossy(path)),
    };
    format!("{reason} ({})", String::from_utf8_lossy(path))
}

#[cfg(test)]
mod tests;

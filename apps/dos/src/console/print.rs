//! The DOS prompt's background text-file print queue.

use super::{command_line::Word, file_error};
use futures_lite::future::yield_now;
use relocade_dos::{devices::CharacterDevice, ConsoleFile, DosError, NativeConsole};
use std::{collections::VecDeque, future::Future, pin::Pin, rc::Rc};

const MAX_JOBS: usize = 10;

struct Job {
    name: Vec<u8>,
    // Keep a partially accepted driver write alive while a command uses the prompt.
    transfer: Pin<Box<dyn Future<Output = Result<(), DosError>>>>,
}

#[derive(Default)]
pub(super) struct PrintQueue {
    jobs: VecDeque<Job>,
}

impl PrintQueue {
    pub fn command(
        &mut self,
        console: &NativeConsole<'_>,
        files: &[Word<'_>],
    ) -> Result<Vec<u8>, String> {
        if files.is_empty() {
            if self.jobs.is_empty() {
                return Ok(b"Print queue is empty\r\n".to_vec());
            }
            let mut listing = Vec::new();
            for job in &self.jobs {
                listing.extend_from_slice(&job.name);
                listing.extend_from_slice(b"\r\n");
            }
            return Ok(listing);
        }
        if matches!(files, [argument] if argument.value.eq_ignore_ascii_case(b"/T")) {
            self.jobs.clear();
            return Ok(b"Print queue cleared\r\n".to_vec());
        }
        if files.iter().any(|word| {
            word.value.starts_with(b"/") || word.value.iter().any(|b| matches!(b, b'*' | b'?'))
        }) {
            return Err("Usage: PRINT [file ...] or PRINT /T".into());
        }
        if files.len() > MAX_JOBS - self.jobs.len() {
            return Err("Print queue is full".into());
        }

        let printer = console
            .device(b"PRN")
            .ok_or("No DOS printer is installed")?;
        let mut new_jobs = Vec::with_capacity(files.len());
        for word in files {
            let input = console
                .open_input_file(&word.value)
                .map_err(|error| file_error(&word.value, error))?;
            new_jobs.push(Job {
                name: word.value.clone(),
                transfer: Box::pin(print_file(input, printer.clone())),
            });
        }
        self.jobs.extend(new_jobs);
        Ok(Vec::new())
    }

    pub(super) async fn drain(&mut self, failures: &mut Vec<String>) {
        while let Some(job) = self.jobs.front_mut() {
            if let Err(error) = job.transfer.as_mut().await {
                failures.push(format!(
                    "Error printing {} ({error})",
                    String::from_utf8_lossy(&job.name)
                ));
            }
            self.jobs.pop_front();
        }
    }
}

async fn print_file(file: ConsoleFile, printer: Rc<CharacterDevice>) -> Result<(), DosError> {
    let mut input = [0u8; 512];
    let mut output = Vec::with_capacity(input.len());
    let mut column = 0u8;
    loop {
        let count = file.read(&mut input).await?;
        let bytes = &input[..count];
        let end = bytes.iter().position(|&byte| byte == 0x1a);
        output.clear();
        for &byte in &bytes[..end.unwrap_or(count)] {
            match byte {
                b'\t' => {
                    let spaces = 8 - column % 8;
                    output.extend_from_slice(&b"        "[..usize::from(spaces)]);
                    column = column.wrapping_add(spaces);
                }
                _ => {
                    column = match byte {
                        b'\r' => 0,
                        8 => column.wrapping_sub(1),
                        0x20..=0xff => column.wrapping_add(1),
                        _ => column,
                    };
                    output.push(byte);
                }
            }
        }
        if count == 0 || end.is_some() {
            output.push(0x0c);
            return printer.write_all(&output).await;
        }
        printer.write_all(&output).await?;
        yield_now().await;
    }
}

#[cfg(test)]
mod tests;

//! DOS character services. Arguments are captured before any I/O can suspend.

use super::control::Request;
use crate::{console::LineEditor, devices::CONTROL_C, DosError, ServiceError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, set_zero},
    CpuContext, FarPointer,
};

mod request;
pub(super) use request::{check_console_break, Input};

#[interrupt(context(call), args(input_function = al, byte = dl, buffer = ptr(ds, dx)), returns {
    Ok(()) => {},
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn dispatch(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    input_function: u8,
    byte: u8,
    buffer: FarPointer,
) -> Result<(), DosError> {
    let cpu = call.cpu;
    let function = request.function;
    'operation: loop {
        // A break starts a new attempt. Ordinary waits and IRQs keep these handles.
        let input = Input::new(request.services);
        let function = if function == 0x0c {
            // AH=0Ch repeats its flush when INT23 restarts the operation. A failed
            // flush still proceeds to the operation selected by AL.
            let stdin = request.services.process.files().handle(0).cloned();
            if let Ok(stdin) = stdin {
                let _ = stdin.flush_input().await;
            }
            match input_function {
                0x01 | 0x06 | 0x07 | 0x08 | 0x0a => input_function,
                _ => {
                    cpu.set_al(0);
                    return Ok(());
                }
            }
        } else {
            function
        };
        match function {
            0x01 | 0x07 | 0x08 => {
                let checked = function != 0x07;
                let byte = input.character(checked).await?;
                if checked && byte == CONTROL_C {
                    request.handle_break(call).await;
                    continue;
                }
                if function == 0x01 {
                    input.write(byte).await;
                }
                cpu.set_al(byte);
            }
            0x02 => {
                if input.break_pending().await {
                    request.handle_break(call).await;
                    continue;
                }
                input.write(byte).await;
                cpu.set_al(if byte == b'\t' { b' ' } else { byte });
            }
            0x06 if byte == 0xff => {
                let byte = input.poll_character().await?;
                cpu.set_al(byte.unwrap_or(0));
                set_zero(cpu, &mut call.memory, byte.is_none());
            }
            0x06 => {
                input.write_raw(byte).await;
                cpu.set_al(byte);
            }
            0x09 => {
                for index in 0..=u16::MAX {
                    let mut byte = [0];
                    call.memory
                        .read_wrapping(buffer.wrapping_add(usize::from(index)), &mut byte)
                        .unwrap();
                    if byte[0] == b'$' {
                        cpu.set_al(b'$');
                        return Ok(());
                    }
                    if input.break_pending().await {
                        request.handle_break(call).await;
                        continue 'operation;
                    }
                    input.write(byte[0]).await;
                }
                request
                    .services
                    .exit(&mut call.memory, Err(ServiceError::UnterminatedString));
            }
            0x0a => {
                let capacity = call.memory.read_u8(buffer.address()).unwrap();
                if capacity != 0 {
                    let mut editor = LineEditor::new(capacity, async |byte| {
                        input.write(byte).await;
                        Ok::<_, DosError>(())
                    });
                    loop {
                        let byte = input.character(true).await?;
                        if byte == CONTROL_C {
                            request.handle_break(call).await;
                            continue 'operation;
                        }
                        if editor.accept(byte).await? {
                            break;
                        }
                    }
                    let bytes = editor.finish();
                    // DOS's capacity includes CR, but the returned count excludes it.
                    call.memory
                        .write_wrapping(buffer.wrapping_add(1), &[(bytes.len() - 1) as u8])
                        .unwrap();
                    call.memory
                        .write_wrapping(buffer.wrapping_add(2), &bytes)
                        .unwrap();
                }
            }
            0x0b => {
                if input.break_pending().await {
                    request.handle_break(call).await;
                    continue;
                }
                cpu.set_al(if input.status().await { 0xff } else { 0 });
            }
            _ => request.services.exit(
                &mut call.memory,
                Err(ServiceError::UnsupportedFunction(function)),
            ),
        }
        return Ok(());
    }
}

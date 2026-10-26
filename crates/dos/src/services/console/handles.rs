//! Console handle reads, including cooked lines and raw byte transfers.

use super::{
    line::LineInput,
    output::{write_console, CharacterOutput},
};
use crate::{
    interrupt::return_result, services::buffers::transfer_count, stdio::OutputStream, DosServices,
};
use relocade_machine::{CpuContext, HookResult};
use relocade_x86::Segment;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: keep the input record and destination fixed throughout a pending read.
pub(in crate::services) async fn read_handle(
    call: &mut CpuContext<'_, DosServices>,
    services: &DosServices,
    stream: OutputStream,
) -> HookResult {
    let cpu = &call.cpu;
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let handle = cpu.bx();
    let input = match services.input(handle) {
        Ok(input) => input,
        Err(error) => return return_result(call, Err(error)),
    };
    let device = input.console().expect("validated console input");
    let bytes = if device.eof() {
        Vec::new()
    } else if device.raw() {
        let mut bytes = Vec::with_capacity(count);
        while bytes.len() < count {
            match input.read_character().await {
                Ok(byte) => bytes.push(byte),
                Err(error) => return return_result(call, Err(error)),
            }
        }
        bytes
    } else {
        let buffered = !services.borrow_mut().stdio.console_line.is_empty();
        if count != 0 && !buffered {
            let line = LineInput::new(CONSOLE_LINE_CAPACITY, CharacterOutput::Console(stream));
            let mut bytes = match line.read(call, services, &input).await {
                Ok(bytes) => bytes,
                Err(error) => return return_result(call, Err(error)),
            };
            let eof = bytes.first() == Some(&0x1a);
            device.set_eof(eof);
            let state = &mut *services.borrow_mut();
            write_console(call, state, stream, b"\n");
            if !eof {
                bytes.push(b'\n');
                state.stdio.console_line.extend(bytes);
            }
        }
        let state = &mut *services.borrow_mut();
        let line = &mut state.stdio.console_line;
        line.drain(..count.min(line.len())).collect()
    };
    if !bytes.is_empty() {
        call.memory
            .write_segment(&segment, u32::from(offset), &bytes)
            .expect("real-mode console read buffer");
    }
    call.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}

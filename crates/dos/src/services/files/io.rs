//! Bind DOS handle transfers to captured register arguments and guest memory.

use super::transfer;
use crate::{
    interrupt::{return_result, DosContext},
    services::buffers::transfer_count,
};
use relocade_machine::HookResult;
use relocade_x86::Segment;

/// AH=3Fh: retain the source and DS:DX destination throughout a pending read.
pub(in crate::services) async fn read(call: &mut DosContext<'_>) -> HookResult {
    let cpu = &call.cpu;
    let handle = cpu.bx();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let source = match call.process.files().readable_handle(handle) {
        Ok(source) => source,
        Err(error) => return return_result(call, Err(error)),
    };
    let bytes = match transfer::read(call.console(), &source, count).await {
        Ok(bytes) => bytes,
        Err(error) => return return_result(call, Err(error)),
    };
    if !bytes.is_empty() {
        call.memory
            .write_segment(&segment, u32::from(offset), &bytes)
            .expect("real-mode read buffer");
    }
    call.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}

/// AH=40h: write CX bytes to BX; CX=0 truncates or extends a file at its cursor.
pub(in crate::services) fn write(call: &mut DosContext<'_>) -> HookResult {
    let cpu = &call.cpu;
    let handle = cpu.bx();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    if count != 0 {
        call.memory
            .read_segment(&segment, u32::from(offset), &mut bytes)
            .expect("real-mode write buffer");
    }
    let destination = call.process.files().writable_handle(handle);
    let result = destination
        .and_then(|destination| transfer::write(call.console(), &destination, &bytes))
        .map(|count| call.cpu.set_ax(count as u16));
    return_result(call, result)
}

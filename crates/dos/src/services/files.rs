use crate::{error::DosError, interrupt::return_result, path::MAX_PATH_BYTES, DosState};
use relocade_runtime::HostCall;
use relocade_x86::Segment;
use std::ops::ControlFlow;

/// AH=71h: report that long-filename services are absent so callers can use 8.3.
pub(super) fn long_filename(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    return_result(call, Err(DosError::LongFilenameNotSupported))
}

/// AH=3Ch: create or truncate the path at DS:DX with CX's attributes; return AX.
pub(super) fn create(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let attributes = call.cpu().cx();
    let result = read_path(call).and_then(|path| call.state_mut().files.create(&path, attributes));
    let result = result.map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=3Dh: open the NUL-terminated path at DS:DX using AL's access mode.
pub(super) fn open(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let mode = call.cpu().al();
    let result = read_path(call).and_then(|path| call.state_mut().files.open(&path, mode));
    let result = result.map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=3Eh: close BX's file handle.
pub(super) fn close(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let handle = call.cpu().bx();
    let result = call.state_mut().files.close(handle);
    return_result(call, result)
}

/// AH=45h: duplicate BX's open handle into the lowest free slot; return AX.
pub(super) fn duplicate(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let source = call.cpu().bx();
    let result = call
        .state_mut()
        .files
        .duplicate(source)
        .map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=46h: replace CX's handle with a duplicate of BX, sharing its file position.
pub(super) fn force_duplicate(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let source = call.cpu().bx();
    let destination = call.cpu().cx();
    let result = call.state_mut().files.force_duplicate(source, destination);
    return_result(call, result)
}

/// AH=3Fh: read up to CX bytes from BX into DS:DX; return the count in AX.
pub(super) fn read(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let cpu = call.cpu();
    let handle = cpu.bx();
    let count = usize::from(cpu.cx());
    let address = cpu.segment(Segment::Ds).base + u32::from(cpu.dx());
    let mut bytes = vec![0; count];
    let result = call
        .state_mut()
        .files
        .read(handle, &mut bytes)
        .map(|count| {
            call.memory()
                .write(address, &bytes[..count])
                .expect("real-mode buffer");
            call.cpu().set_ax(count as u16);
        });
    return_result(call, result)
}

/// AH=40h: write CX bytes to BX; CX=0 truncates or extends a file at its cursor.
pub(super) fn write(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let handle = call.cpu().bx();
    let cpu = call.cpu();
    let count = usize::from(cpu.cx());
    let address = cpu.segment(Segment::Ds).base + u32::from(cpu.dx());
    let mut bytes = vec![0; count];
    if count != 0 {
        call.memory()
            .read(address, &mut bytes)
            .expect("real-mode buffer");
    }
    let result = call
        .state_mut()
        .files
        .write(handle, &bytes)
        .map(|count| call.cpu().set_ax(count as u16));
    return_result(call, result)
}

/// AH=42h: seek BX by CX:DX from AL's origin; return DX:AX.
pub(super) fn seek(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let cpu = call.cpu();
    let handle = cpu.bx();
    let origin = cpu.al();
    let offset = (u32::from(cpu.cx()) << 16) | u32::from(cpu.dx());
    let result = call
        .state_mut()
        .files
        .seek(handle, origin, offset)
        .map(|position| {
            call.cpu().set_ax(position as u16);
            call.cpu().set_dx((position >> 16) as u16);
        });
    return_result(call, result)
}

pub(super) fn read_path(call: &mut HostCall<'_, DosState>) -> Result<Vec<u8>, DosError> {
    let cpu = call.cpu();
    let base = cpu.segment(Segment::Ds).base;
    let offset = cpu.dx();
    let mut path = Vec::new();
    for index in 0..MAX_PATH_BYTES as u16 {
        let mut byte = [0];
        call.memory()
            .read(base + u32::from(offset.wrapping_add(index)), &mut byte)
            .expect("real-mode path address");
        if byte[0] == 0 {
            return Ok(path);
        }
        path.push(byte[0]);
    }
    Err(DosError::PathNotFound)
}

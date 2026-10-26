use super::buffers::{transfer_count, write_wrapping};
use crate::{
    error::DosError,
    files::{CreateMode, Device, HandleType},
    interrupt::{return_result, DosContext},
    path::MAX_PATH_BYTES,
};
use relocade_machine::HookResult;
use relocade_x86::Segment;

/// AH=71h: report that long-filename services are absent so callers can use 8.3.
pub(super) fn long_filename(call: &mut DosContext<'_, '_>) -> HookResult {
    return_result(call, Err(DosError::LongFilenameNotSupported))
}

/// AH=43h: query (AL=0) or set (AL=1) DS:DX's attributes through CX.
pub(super) fn attributes(call: &mut DosContext<'_, '_>) -> HookResult {
    let operation = call.execution.cpu.al();
    if operation > 1 {
        return return_result(call, Err(DosError::InvalidFunction));
    }
    let attributes = call.execution.cpu.cx();
    let result = read_path(call).and_then(|path| match operation {
        0 => call.dos.filesystem().attributes(&path).map(|attributes| {
            call.execution.cpu.set_cx(attributes);
        }),
        _ => call.dos.filesystem().set_attributes(&path, attributes),
    });
    return_result(call, result)
}

/// AH=41h: delete the file at DS:DX, subject to DOS attributes and sharing.
pub(super) fn delete(call: &mut DosContext<'_, '_>) -> HookResult {
    let result = read_path(call).and_then(|path| {
        call.process
            .files()
            .delete(&mut call.dos.filesystem(), &path)
    });
    return_result(call, result)
}

/// AH=56h: rename DS:DX's file to ES:DI's path on the same drive.
pub(super) fn rename(call: &mut DosContext<'_, '_>) -> HookResult {
    let offset = call.execution.cpu.di();
    let result = read_path(call).and_then(|from| {
        let to = read_path_at(call, Segment::Es, offset)?;
        call.process
            .files()
            .rename(&mut call.dos.filesystem(), &from, &to)
    });
    return_result(call, result)
}

/// AH=3Ch/5Bh: create DS:DX with CX's attributes using the requested mode; return AX.
pub(super) fn create(call: &mut DosContext<'_, '_>, mode: CreateMode) -> HookResult {
    let attributes = call.execution.cpu.cx();
    let result = read_path(call).and_then(|path| {
        call.process
            .files()
            .create(&mut call.dos.filesystem(), &path, attributes, mode)
    });
    let result = result.map(|handle| call.execution.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=5Ah: create a temporary file in DS:DX's directory and return its path and AX.
pub(super) fn create_temporary(call: &mut DosContext<'_, '_>) -> HookResult {
    let attributes = call.execution.cpu.cx();
    let result = read_path(call).and_then(|mut path| {
        let handle = call.process.files().create_temporary(
            &mut call.dos.filesystem(),
            &mut path,
            attributes,
        )?;
        let cpu = &call.execution.cpu;
        let segment = cpu.segment(Segment::Ds);
        let offset = cpu.dx();
        path.push(0);
        write_wrapping(&mut call.execution.memory, &segment, offset, &path);
        call.execution.cpu.set_ax(handle);
        Ok(())
    });
    return_result(call, result)
}

/// AH=3Dh: open the NUL-terminated path at DS:DX using AL's access mode.
pub(super) fn open(call: &mut DosContext<'_, '_>) -> HookResult {
    let mode = call.execution.cpu.al();
    let result = read_path(call).and_then(|path| {
        call.process
            .files()
            .open(&call.dos.filesystem(), &path, mode)
    });
    let result = result.map(|handle| call.execution.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=3Eh: close BX's file handle.
pub(super) fn close(call: &mut DosContext<'_, '_>) -> HookResult {
    let handle = call.execution.cpu.bx();
    let result = call
        .process
        .files()
        .close(&mut call.dos.filesystem(), handle);
    return_result(call, result)
}

/// AH=68h/6Ah: commit BX's handle without closing it or moving its cursor.
pub(super) fn commit(call: &mut DosContext<'_, '_>) -> HookResult {
    let handle = call.execution.cpu.bx();
    let result = call
        .process
        .files()
        .commit(&mut call.dos.filesystem(), handle)
        .map(|()| call.execution.cpu.set_ah(super::COMMIT_HANDLE));
    return_result(call, result)
}

/// AH=45h: duplicate BX's open handle into the lowest free slot; return AX.
pub(super) fn duplicate(call: &mut DosContext<'_, '_>) -> HookResult {
    let source = call.execution.cpu.bx();
    let result = call
        .process
        .files()
        .duplicate(source)
        .map(|handle| call.execution.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=46h: replace CX's handle with a duplicate of BX, sharing its file position.
pub(super) fn force_duplicate(call: &mut DosContext<'_, '_>) -> HookResult {
    let source = call.execution.cpu.bx();
    let destination = call.execution.cpu.cx();
    let result =
        call.process
            .files()
            .force_duplicate(&mut call.dos.filesystem(), source, destination);
    return_result(call, result)
}

/// AH=3Fh: read up to CX bytes from BX into DS:DX; return the count in AX.
pub(super) fn read(call: &mut DosContext<'_, '_>) -> HookResult {
    let handle = call.execution.cpu.bx();
    let kind = call.process.files().readable_handle_type(handle);
    match kind {
        Ok(HandleType::Device(Device::Console(stream))) => {
            return super::console::read_handle(call, stream);
        }
        Ok(HandleType::File | HandleType::Device(Device::Null)) => {}
        Ok(HandleType::Device(Device::Auxiliary | Device::Printer)) => {
            return return_result(call, Err(DosError::AccessDenied));
        }
        Err(error) => return return_result(call, Err(error)),
    }
    let cpu = &call.execution.cpu;
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    let result = call.process.files().read(handle, &mut bytes).map(|count| {
        call.execution
            .memory
            .write_segment(&segment, u32::from(offset), &bytes[..count])
            .expect("real-mode buffer");
        call.execution.cpu.set_ax(count as u16);
    });
    return_result(call, result)
}

/// AH=40h: write CX bytes to BX; CX=0 truncates or extends a file at its cursor.
pub(super) fn write(call: &mut DosContext<'_, '_>) -> HookResult {
    let handle = call.execution.cpu.bx();
    let cpu = &call.execution.cpu;
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    if count != 0 {
        call.execution
            .memory
            .read_segment(&segment, u32::from(offset), &mut bytes)
            .expect("real-mode buffer");
    }
    let result = super::console::write_handle(call, handle, &bytes)
        .map(|count| call.execution.cpu.set_ax(count as u16));
    return_result(call, result)
}

/// AH=42h: seek BX by CX:DX from AL's origin; return DX:AX.
pub(super) fn seek(call: &mut DosContext<'_, '_>) -> HookResult {
    let cpu = &call.execution.cpu;
    let handle = cpu.bx();
    let origin = cpu.al();
    let offset = (u32::from(cpu.cx()) << 16) | u32::from(cpu.dx());
    let result = call
        .process
        .files()
        .seek(handle, origin, offset)
        .map(|position| {
            call.execution.cpu.set_ax(position as u16);
            call.execution.cpu.set_dx((position >> 16) as u16);
        });
    return_result(call, result)
}

pub(super) fn read_path(call: &mut DosContext<'_, '_>) -> Result<Vec<u8>, DosError> {
    let offset = call.execution.cpu.dx();
    read_path_at(call, Segment::Ds, offset)
}

fn read_path_at(
    call: &mut DosContext<'_, '_>,
    segment: Segment,
    offset: u16,
) -> Result<Vec<u8>, DosError> {
    let segment = call.execution.cpu.segment(segment);
    let mut path = Vec::new();
    for index in 0..MAX_PATH_BYTES as u16 {
        let mut byte = [0];
        call.execution
            .memory
            .read_segment(&segment, u32::from(offset.wrapping_add(index)), &mut byte)
            .expect("real-mode path address");
        if byte[0] == 0 {
            return Ok(path);
        }
        path.push(byte[0]);
    }
    Err(DosError::PathNotFound)
}

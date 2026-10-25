use super::buffers::{transfer_count, write_wrapping};
use crate::{
    error::DosError,
    files::{CreateMode, Device, HandleType},
    interrupt::return_result,
    path::MAX_PATH_BYTES,
    state::DosState,
    DosServices,
};
use relocade_machine::{CpuContext, HookResult};
use relocade_x86::Segment;

/// AH=71h: report that long-filename services are absent so callers can use 8.3.
pub(super) fn long_filename(call: &mut CpuContext<'_, DosServices>) -> HookResult {
    return_result(call, Err(DosError::LongFilenameNotSupported))
}

/// AH=43h: query (AL=0) or set (AL=1) DS:DX's attributes through CX.
pub(super) fn attributes(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let operation = call.cpu().al();
    if operation > 1 {
        return return_result(call, Err(DosError::InvalidFunction));
    }
    let attributes = call.cpu().cx();
    let result = read_path(call).and_then(|path| match operation {
        0 => state.files.attributes(&path).map(|attributes| {
            call.cpu().set_cx(attributes);
        }),
        _ => state.files.set_attributes(&path, attributes),
    });
    return_result(call, result)
}

/// AH=41h: delete the file at DS:DX, subject to DOS attributes and sharing.
pub(super) fn delete(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let result = read_path(call).and_then(|path| state.files.delete(&path));
    return_result(call, result)
}

/// AH=56h: rename DS:DX's file to ES:DI's path on the same drive.
pub(super) fn rename(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let offset = call.cpu().di();
    let result = read_path(call).and_then(|from| {
        let to = read_path_at(call, Segment::Es, offset)?;
        state.files.rename(&from, &to)
    });
    return_result(call, result)
}

/// AH=3Ch/5Bh: create DS:DX with CX's attributes using the requested mode; return AX.
pub(super) fn create(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    mode: CreateMode,
) -> HookResult {
    let attributes = call.cpu().cx();
    let result = read_path(call).and_then(|path| state.files.create(&path, attributes, mode));
    let result = result.map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=5Ah: create a temporary file in DS:DX's directory and return its path and AX.
pub(super) fn create_temporary(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let attributes = call.cpu().cx();
    let result = read_path(call).and_then(|mut path| {
        let handle = state.files.create_temporary(&mut path, attributes)?;
        let cpu = call.cpu();
        let segment = cpu.segment(Segment::Ds);
        let offset = cpu.dx();
        path.push(0);
        write_wrapping(&mut call.memory(), &segment, offset, &path);
        call.cpu().set_ax(handle);
        Ok(())
    });
    return_result(call, result)
}

/// AH=3Dh: open the NUL-terminated path at DS:DX using AL's access mode.
pub(super) fn open(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let mode = call.cpu().al();
    let result = read_path(call).and_then(|path| state.files.open(&path, mode));
    let result = result.map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=3Eh: close BX's file handle.
pub(super) fn close(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let handle = call.cpu().bx();
    return_result(call, state.files.close(handle))
}

/// AH=68h/6Ah: commit BX's handle without closing it or moving its cursor.
pub(super) fn commit(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let handle = call.cpu().bx();
    let result = state
        .files
        .commit(handle)
        .map(|()| call.cpu().set_ah(super::COMMIT_HANDLE));
    return_result(call, result)
}

/// AH=45h: duplicate BX's open handle into the lowest free slot; return AX.
pub(super) fn duplicate(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let source = call.cpu().bx();
    let result = state
        .files
        .duplicate(source)
        .map(|handle| call.cpu().set_ax(handle));
    return_result(call, result)
}

/// AH=46h: replace CX's handle with a duplicate of BX, sharing its file position.
pub(super) fn force_duplicate(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let source = call.cpu().bx();
    let destination = call.cpu().cx();
    return_result(call, state.files.force_duplicate(source, destination))
}

/// AH=3Fh: read up to CX bytes from BX into DS:DX; return the count in AX.
pub(super) async fn read(
    call: &mut CpuContext<'_, DosServices>,
    services: &DosServices,
) -> HookResult {
    let handle = call.cpu().bx();
    let kind = services.borrow_mut().files.readable_handle_type(handle);
    match kind {
        Ok(HandleType::Device(Device::Console(stream))) => {
            return super::console::read_handle(call, services, stream).await;
        }
        Ok(HandleType::File | HandleType::Device(Device::Null)) => {}
        Ok(HandleType::Device(Device::Auxiliary | Device::Printer)) => {
            return return_result(call, Err(DosError::AccessDenied));
        }
        Err(error) => return return_result(call, Err(error)),
    }
    let cpu = call.cpu();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    let result = services
        .borrow_mut()
        .files
        .read(handle, &mut bytes)
        .map(|count| {
            call.memory()
                .write_segment(&segment, u32::from(offset), &bytes[..count])
                .expect("real-mode buffer");
            call.cpu().set_ax(count as u16);
        });
    return_result(call, result)
}

/// AH=40h: write CX bytes to BX; CX=0 truncates or extends a file at its cursor.
pub(super) fn write(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let handle = call.cpu().bx();
    let cpu = call.cpu();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    if count != 0 {
        call.memory()
            .read_segment(&segment, u32::from(offset), &mut bytes)
            .expect("real-mode buffer");
    }
    let result = super::console::write_handle(call, state, handle, &bytes)
        .map(|count| call.cpu().set_ax(count as u16));
    return_result(call, result)
}

/// AH=42h: seek BX by CX:DX from AL's origin; return DX:AX.
pub(super) fn seek(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let cpu = call.cpu();
    let handle = cpu.bx();
    let origin = cpu.al();
    let offset = (u32::from(cpu.cx()) << 16) | u32::from(cpu.dx());
    let result = state.files.seek(handle, origin, offset).map(|position| {
        call.cpu().set_ax(position as u16);
        call.cpu().set_dx((position >> 16) as u16);
    });
    return_result(call, result)
}

pub(super) fn read_path(call: &mut CpuContext<'_, DosServices>) -> Result<Vec<u8>, DosError> {
    let offset = call.cpu().dx();
    read_path_at(call, Segment::Ds, offset)
}

fn read_path_at(
    call: &mut CpuContext<'_, DosServices>,
    segment: Segment,
    offset: u16,
) -> Result<Vec<u8>, DosError> {
    let segment = call.cpu().segment(segment);
    let mut path = Vec::new();
    for index in 0..MAX_PATH_BYTES as u16 {
        let mut byte = [0];
        call.memory()
            .read_segment(&segment, u32::from(offset.wrapping_add(index)), &mut byte)
            .expect("real-mode path address");
        if byte[0] == 0 {
            return Ok(path);
        }
        path.push(byte[0]);
    }
    Err(DosError::PathNotFound)
}

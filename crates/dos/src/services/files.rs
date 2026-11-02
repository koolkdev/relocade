use super::Services;
use crate::{error::DosError, files::CreateMode, interrupt::return_result, path::MAX_PATH_BYTES};
use relocade_machine::real_mode_interrupt::interrupt;
use relocade_machine::{CpuContext, FarPointer};
use relocade_x86::Segment;

pub(super) mod io;
mod transfer;

/// AH=71h: report that long-filename services are absent so callers can use 8.3.
#[interrupt(context(call))]
pub(super) fn long_filename(call: &mut CpuContext<'_>) {
    return_result(call, Err(DosError::LongFilenameNotSupported))
}

/// AH=43h: query (AL=0) or set (AL=1) DS:DX's attributes through CX.
#[interrupt(context(call))]
pub(super) fn attributes(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let operation = call.cpu.al();
    if operation > 1 {
        return return_result(call, Err(DosError::InvalidFunction));
    }
    let attributes = call.cpu.cx();
    let result = read_path(call).and_then(|path| match operation {
        0 => services
            .dos
            .filesystem()
            .attributes(&path)
            .map(|attributes| {
                call.cpu.set_cx(attributes);
            }),
        _ => services.dos.filesystem().set_attributes(&path, attributes),
    });
    return_result(call, result)
}

/// AH=41h: delete the file at DS:DX, subject to DOS attributes and sharing.
#[interrupt(context(call))]
pub(super) fn delete(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let result = read_path(call).and_then(|path| {
        services
            .process
            .files()
            .delete(&mut services.dos.filesystem(), &path)
    });
    return_result(call, result)
}

/// AH=56h: rename DS:DX's file to ES:DI's path on the same drive.
#[interrupt(context(call))]
pub(super) fn rename(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let offset = call.cpu.di();
    let result = read_path(call).and_then(|from| {
        let to = read_path_at(call, Segment::Es, offset)?;
        services
            .process
            .files()
            .rename(&mut services.dos.filesystem(), &from, &to)
    });
    return_result(call, result)
}

/// AH=3Ch/5Bh: create DS:DX with CX's attributes using the requested mode; return AX.
pub(super) fn create(call: &mut CpuContext<'_>, services: &Services<'_>, mode: CreateMode) {
    let attributes = call.cpu.cx();
    let result = read_path(call).and_then(|path| {
        services
            .process
            .files()
            .create(&mut services.dos.filesystem(), &path, attributes, mode)
    });
    let result = result.map(|handle| call.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=5Ah: create a temporary file in DS:DX's directory and return its path and AX.
#[interrupt(context(call))]
pub(super) fn create_temporary(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let attributes = call.cpu.cx();
    let result = read_path(call).and_then(|mut path| {
        let handle = services.process.files().create_temporary(
            &mut services.dos.filesystem(),
            &mut path,
            attributes,
        )?;
        let cpu = call.cpu;
        let destination = FarPointer {
            segment: cpu.segment(Segment::Ds).selector,
            offset: cpu.dx(),
        };
        path.push(0);
        call.memory
            .write_wrapping(destination, &path)
            .expect("temporary file path");
        call.cpu.set_ax(handle);
        Ok(())
    });
    return_result(call, result)
}

/// AH=3Dh: open the NUL-terminated path at DS:DX using AL's access mode.
#[interrupt(context(call))]
pub(super) fn open(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let mode = call.cpu.al();
    let result = read_path(call).and_then(|path| {
        services
            .process
            .files()
            .open(&services.dos.filesystem(), &path, mode)
    });
    let result = result.map(|handle| call.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=3Eh: close BX's file handle.
#[interrupt(context(call))]
pub(super) fn close(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let handle = call.cpu.bx();
    let result = services
        .process
        .files()
        .close(&mut services.dos.filesystem(), handle);
    return_result(call, result)
}

/// AH=68h/6Ah: commit BX's handle without closing it or moving its cursor.
#[interrupt(context(call))]
pub(super) fn commit(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let handle = call.cpu.bx();
    let result = services
        .process
        .files()
        .commit(&mut services.dos.filesystem(), handle)
        .map(|()| call.cpu.set_ah(0x68));
    return_result(call, result)
}

/// AH=45h: duplicate BX's open handle into the lowest free slot; return AX.
#[interrupt(context(call))]
pub(super) fn duplicate(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let source = call.cpu.bx();
    let result = services
        .process
        .files()
        .duplicate(source)
        .map(|handle| call.cpu.set_ax(handle));
    return_result(call, result)
}

/// AH=46h: replace CX's handle with a duplicate of BX, sharing its file position.
#[interrupt(context(call))]
pub(super) fn force_duplicate(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let source = call.cpu.bx();
    let destination = call.cpu.cx();
    let result = services.process.files().force_duplicate(
        &mut services.dos.filesystem(),
        source,
        destination,
    );
    return_result(call, result)
}

/// AH=42h: seek BX by CX:DX from AL's origin; return DX:AX.
#[interrupt(context(call))]
pub(super) fn seek(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let cpu = call.cpu;
    let handle = cpu.bx();
    let origin = cpu.al();
    let offset = (u32::from(cpu.cx()) << 16) | u32::from(cpu.dx());
    let result = services
        .process
        .files()
        .seek(handle, origin, offset)
        .map(|position| {
            call.cpu.set_ax(position as u16);
            call.cpu.set_dx((position >> 16) as u16);
        });
    return_result(call, result)
}

pub(super) fn read_path(call: &mut CpuContext<'_>) -> Result<Vec<u8>, DosError> {
    let offset = call.cpu.dx();
    read_path_at(call, Segment::Ds, offset)
}

fn read_path_at(
    call: &mut CpuContext<'_>,
    segment: Segment,
    offset: u16,
) -> Result<Vec<u8>, DosError> {
    let source = FarPointer {
        segment: call.cpu.segment(segment).selector,
        offset,
    };
    let mut path = Vec::new();
    for index in 0..MAX_PATH_BYTES {
        let mut byte = [0];
        call.memory
            .read_wrapping(source.wrapping_add(index), &mut byte)
            .expect("real-mode path address");
        if byte[0] == 0 {
            return Ok(path);
        }
        path.push(byte[0]);
    }
    Err(DosError::PathNotFound)
}

#[interrupt(context(call))]
pub(super) fn create_or_truncate(call: &mut CpuContext<'_>, services: &Services<'_>) {
    create(call, services, CreateMode::TruncateExisting);
}

#[interrupt(context(call))]
pub(super) fn create_new(call: &mut CpuContext<'_>, services: &Services<'_>) {
    create(call, services, CreateMode::FailIfExists);
}

#[interrupt(context(call))]
pub(super) fn commit_alias(call: &mut CpuContext<'_>, services: &Services<'_>) {
    commit(call, services);
}

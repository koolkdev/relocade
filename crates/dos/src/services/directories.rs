use super::{buffers::write_wrapping, files::read_path};
use crate::{interrupt::return_result, state::DosState, DosServices};
use relocade_runtime::{real_mode_interrupt::return_from_interrupt, HostCall};
use relocade_x86::{ExecutionExit, Segment};
use std::ops::ControlFlow;

/// AH=0Eh: select DL's drive; report the drive-letter range in AL.
pub(super) fn select_drive(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let number = call.cpu().dl();
    let count = state.files.select_drive(number);
    call.cpu().set_al(count);
    return_from_interrupt(call)
}

/// AH=19h: return the current drive in AL.
pub(super) fn get_drive(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let number = state.files.current_drive();
    call.cpu().set_al(number);
    return_from_interrupt(call)
}

/// AH=39h: create the directory at DS:DX; its parent must already exist.
pub(super) fn create_directory(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| state.files.create_directory(&path));
    return_result(call, result)
}

/// AH=3Ah: remove the empty directory at DS:DX, excluding root and current.
pub(super) fn remove_directory(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| state.files.remove_directory(&path));
    return_result(call, result)
}

/// AH=3Bh: change to the NUL-terminated directory at DS:DX.
pub(super) fn change_directory(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| state.files.change_directory(&path));
    return_result(call, result)
}

/// AH=47h: write DL's current directory to DS:SI, without a drive or leading slash.
pub(super) fn get_directory(
    call: &mut HostCall<'_, DosServices>,
    state: &mut DosState,
) -> ControlFlow<ExecutionExit> {
    let cpu = call.cpu();
    let drive = cpu.dl();
    let segment = cpu.segment(Segment::Ds);
    let offset = cpu.si();
    let result = state.files.current_directory(drive).map(|path| {
        let mut bytes = path.replace('/', "\\").into_bytes();
        bytes.push(0);
        write_wrapping(&mut call.memory(), &segment, offset, &bytes);
        call.cpu().set_ax(0x0100);
    });
    return_result(call, result)
}

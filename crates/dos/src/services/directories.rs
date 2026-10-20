use super::files::read_path;
use crate::{interrupt::return_result, DosState};
use relocade_runtime::{real_mode_interrupt::return_from_interrupt, HostCall};
use relocade_x86::{ExecutionExit, Segment};
use std::ops::ControlFlow;

/// AH=0Eh: select DL's drive; report the drive-letter range in AL.
pub(super) fn select_drive(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let number = call.cpu().dl();
    let count = call.state_mut().files.select_drive(number);
    call.cpu().set_al(count);
    return_from_interrupt(call)
}

/// AH=19h: return the current drive in AL.
pub(super) fn get_drive(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let number = call.state().files.current_drive();
    call.cpu().set_al(number);
    return_from_interrupt(call)
}

/// AH=39h: create the directory at DS:DX; its parent must already exist.
pub(super) fn create_directory(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| call.state_mut().files.create_directory(&path));
    return_result(call, result)
}

/// AH=3Ah: remove the empty directory at DS:DX, excluding root and current.
pub(super) fn remove_directory(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| call.state_mut().files.remove_directory(&path));
    return_result(call, result)
}

/// AH=3Bh: change to the NUL-terminated directory at DS:DX.
pub(super) fn change_directory(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let result = read_path(call).and_then(|path| call.state_mut().files.change_directory(&path));
    return_result(call, result)
}

/// AH=47h: write DL's current directory to DS:SI, without a drive or leading slash.
pub(super) fn get_directory(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let cpu = call.cpu();
    let drive = cpu.dl();
    let address = cpu.segment(Segment::Ds).base + u32::from(cpu.si());
    let result = call.state().files.current_directory(drive).map(|path| {
        let mut bytes = path.replace('/', "\\").into_bytes();
        bytes.push(0);
        bytes
    });
    let result = result.map(|bytes| {
        call.memory()
            .write(address, &bytes)
            .expect("real-mode directory buffer");
        call.cpu().set_ax(0x0100);
    });
    return_result(call, result)
}

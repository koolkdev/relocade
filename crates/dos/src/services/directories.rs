use super::{buffers::write_wrapping, files::read_path};
use crate::interrupt::{return_result, DosContext};
use relocade_machine::{real_mode_interrupt::return_from_interrupt, HookResult};
use relocade_x86::Segment;

/// AH=0Eh: select DL's drive; report the drive-letter range in AL.
pub(super) fn select_drive(call: &mut DosContext) -> HookResult {
    let number = call.execution.cpu.dl();
    let count = call.dos.filesystem().select_drive(number);
    call.execution.cpu.set_al(count);
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=19h: return the current drive in AL.
pub(super) fn get_drive(call: &mut DosContext) -> HookResult {
    let number = call.dos.filesystem().current_drive();
    call.execution.cpu.set_al(number);
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=39h: create the directory at DS:DX; its parent must already exist.
pub(super) fn create_directory(call: &mut DosContext) -> HookResult {
    let result = read_path(call).and_then(|path| call.dos.filesystem().create_directory(&path));
    return_result(call, result)
}

/// AH=3Ah: remove the empty directory at DS:DX, excluding root and current.
pub(super) fn remove_directory(call: &mut DosContext) -> HookResult {
    let result = read_path(call).and_then(|path| call.dos.filesystem().remove_directory(&path));
    return_result(call, result)
}

/// AH=3Bh: change to the NUL-terminated directory at DS:DX.
pub(super) fn change_directory(call: &mut DosContext) -> HookResult {
    let result = read_path(call).and_then(|path| call.dos.filesystem().change_directory(&path));
    return_result(call, result)
}

/// AH=47h: write DL's current directory to DS:SI, without a drive or leading slash.
pub(super) fn get_directory(call: &mut DosContext) -> HookResult {
    let cpu = &call.execution.cpu;
    let drive = cpu.dl();
    let segment = cpu.segment(Segment::Ds);
    let offset = cpu.si();
    let result = call.dos.filesystem().current_directory(drive).map(|path| {
        let mut bytes = path.replace('/', "\\").into_bytes();
        bytes.push(0);
        write_wrapping(&mut call.execution.memory.view(), &segment, offset, &bytes);
        call.execution.cpu.set_ax(0x0100);
    });
    return_result(call, result)
}

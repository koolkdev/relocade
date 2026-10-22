use super::{buffers::write_wrapping, files::read_path};
use crate::{interrupt::return_result, state::DosState};
use relocade_machine::{real_mode_interrupt::return_from_interrupt, CpuContext, HostAction};
use relocade_x86::Segment;
use std::rc::Rc;

/// AH=0Eh: select DL's drive; report the drive-letter range in AL.
pub(super) fn select_drive(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let number = call.cpu().dl();
    let count = state.files.borrow_mut().select_drive(number);
    call.cpu().set_al(count);
    return_from_interrupt(call)
}

/// AH=19h: return the current drive in AL.
pub(super) fn get_drive(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let number = state.files.borrow().current_drive();
    call.cpu().set_al(number);
    return_from_interrupt(call)
}

/// AH=39h: create the directory at DS:DX; its parent must already exist.
pub(super) fn create_directory(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let result = read_path(call).and_then(|path| state.files.borrow_mut().create_directory(&path));
    return_result(call, result)
}

/// AH=3Ah: remove the empty directory at DS:DX, excluding root and current.
pub(super) fn remove_directory(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let result = read_path(call).and_then(|path| state.files.borrow_mut().remove_directory(&path));
    return_result(call, result)
}

/// AH=3Bh: change to the NUL-terminated directory at DS:DX.
pub(super) fn change_directory(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let result = read_path(call).and_then(|path| state.files.borrow_mut().change_directory(&path));
    return_result(call, result)
}

/// AH=47h: write DL's current directory to DS:SI, without a drive or leading slash.
pub(super) fn get_directory(call: &mut CpuContext<'_>, state: &Rc<DosState>) -> HostAction {
    let cpu = call.cpu();
    let drive = cpu.dl();
    let segment = cpu.segment(Segment::Ds);
    let offset = cpu.si();
    let result = state.files.borrow().current_directory(drive).map(|path| {
        let mut bytes = path.replace('/', "\\").into_bytes();
        bytes.push(0);
        write_wrapping(&mut call.memory(), &segment, offset, &bytes);
        call.cpu().set_ax(0x0100);
    });
    return_result(call, result)
}

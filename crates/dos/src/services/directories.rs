use super::{files::read_path, Services};
use crate::interrupt::return_result;
use relocade_machine::real_mode_interrupt::interrupt;
use relocade_machine::{CpuContext, FarPointer};
use relocade_x86::Segment;

/// AH=0Eh: select DL's drive; report the drive-letter range in AL.
#[interrupt(context(call))]
pub(super) fn select_drive(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let number = call.cpu.dl();
    let count = services.dos.filesystem().select_drive(number);
    call.cpu.set_al(count);
}

/// AH=19h: return the current drive in AL.
#[interrupt(context(call))]
pub(super) fn get_drive(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let number = services.dos.filesystem().current_drive();
    call.cpu.set_al(number);
}

/// AH=39h: create the directory at DS:DX; its parent must already exist.
#[interrupt(context(call))]
pub(super) fn create_directory(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let result = read_path(call).and_then(|path| services.dos.filesystem().create_directory(&path));
    return_result(call, result)
}

/// AH=3Ah: remove the empty directory at DS:DX, excluding root and current.
#[interrupt(context(call))]
pub(super) fn remove_directory(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let result = read_path(call).and_then(|path| services.dos.filesystem().remove_directory(&path));
    return_result(call, result)
}

/// AH=3Bh: change to the NUL-terminated directory at DS:DX.
#[interrupt(context(call))]
pub(super) fn change_directory(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let result = read_path(call).and_then(|path| services.dos.filesystem().change_directory(&path));
    return_result(call, result)
}

/// AH=47h: write DL's current directory to DS:SI, without a drive or leading slash.
#[interrupt(context(call))]
pub(super) fn get_directory(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let cpu = call.cpu;
    let drive = cpu.dl();
    let destination = FarPointer {
        segment: cpu.segment(Segment::Ds).selector,
        offset: cpu.si(),
    };
    let result = services
        .dos
        .filesystem()
        .current_directory(drive)
        .map(|path| {
            let mut bytes = path.replace('/', "\\").into_bytes();
            bytes.push(0);
            call.memory
                .write_wrapping(destination, &bytes)
                .expect("current directory buffer");
            call.cpu.set_ax(0x0100);
        });
    return_result(call, result)
}

//! DOS device information, mode control and readiness for open handles.

use crate::{error::DosError, interrupt::return_result, state::DosState, DosServices};
use relocade_machine::{CpuContext, HookResult};

const GET_DEVICE_INFORMATION: u8 = 0x00;
const SET_DEVICE_INFORMATION: u8 = 0x01;
const GET_INPUT_STATUS: u8 = 0x06;
const GET_OUTPUT_STATUS: u8 = 0x07;

/// Query or set BX's device information, or query its input/output readiness.
pub(super) fn ioctl(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let handle = call.cpu.bx();
    let result = match call.cpu.al() {
        GET_DEVICE_INFORMATION => state.files.device_information(handle).map(|info| {
            call.cpu.set_dx(info);
            call.cpu.set_ax(info);
        }),
        SET_DEVICE_INFORMATION => {
            let info = call.cpu.dx();
            state
                .files
                .set_device_information(handle, info)
                .map(|previous| {
                    call.cpu.set_al(previous);
                })
        }
        GET_INPUT_STATUS => state.files.input_available(handle).map(|available| {
            call.cpu.set_al(if available { 0xff } else { 0 });
        }),
        GET_OUTPUT_STATUS => state.files.output_ready(handle).map(|ready| {
            call.cpu.set_al(if ready { 0xff } else { 0 });
        }),
        _ => Err(DosError::InvalidFunction),
    };
    return_result(call, result)
}

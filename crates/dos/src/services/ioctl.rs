//! DOS device information, mode control and readiness for open handles.

use crate::{
    error::DosError,
    interrupt::{return_result, DosContext},
};
use relocade_machine::HookResult;

const GET_DEVICE_INFORMATION: u8 = 0x00;
const SET_DEVICE_INFORMATION: u8 = 0x01;
const GET_INPUT_STATUS: u8 = 0x06;
const GET_OUTPUT_STATUS: u8 = 0x07;

/// Query or set BX's device information, or query its input/output readiness.
pub(super) fn ioctl(call: &mut DosContext<'_, '_>) -> HookResult {
    let handle = call.execution.cpu.bx();
    let result = match call.execution.cpu.al() {
        GET_DEVICE_INFORMATION => call.process.files().device_information(handle).map(|info| {
            call.execution.cpu.set_dx(info);
            call.execution.cpu.set_ax(info);
        }),
        SET_DEVICE_INFORMATION => {
            let info = call.execution.cpu.dx();
            call.process
                .files()
                .set_device_information(handle, info)
                .map(|previous| {
                    call.execution.cpu.set_al(previous);
                })
        }
        GET_INPUT_STATUS => call
            .process
            .files()
            .input_available(&call.dos.stdio.input, handle)
            .map(|available| {
                call.execution.cpu.set_al(if available { 0xff } else { 0 });
            }),
        GET_OUTPUT_STATUS => call.process.files().output_ready(handle).map(|ready| {
            call.execution.cpu.set_al(if ready { 0xff } else { 0 });
        }),
        _ => Err(DosError::InvalidFunction),
    };
    return_result(call, result)
}

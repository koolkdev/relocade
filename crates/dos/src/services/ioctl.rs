//! DOS device-control queries for open handles.

use crate::{error::DosError, interrupt::return_result, DosState};
use relocade_runtime::HostCall;
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

const GET_DEVICE_INFORMATION: u8 = 0x00;
const GET_INPUT_STATUS: u8 = 0x06;
const GET_OUTPUT_STATUS: u8 = 0x07;

/// Query BX's file/device information or input/output readiness.
pub(super) fn ioctl(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let handle = call.cpu().bx();
    let result = match call.cpu().al() {
        GET_DEVICE_INFORMATION => call.state().files.device_information(handle).map(|info| {
            call.cpu().set_dx(info);
            call.cpu().set_ax(info);
        }),
        GET_INPUT_STATUS => call.state().files.input_available(handle).map(|available| {
            call.cpu().set_al(if available { 0xff } else { 0 });
        }),
        GET_OUTPUT_STATUS => call.state().files.output_ready(handle).map(|ready| {
            call.cpu().set_al(if ready { 0xff } else { 0 });
        }),
        _ => Err(DosError::InvalidFunction),
    };
    return_result(call, result)
}

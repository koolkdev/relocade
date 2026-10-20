//! DOS device-control queries for open handles.

use crate::{error::DosError, interrupt::return_result, DosState};
use relocade_runtime::HostCall;
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

const GET_DEVICE_INFORMATION: u8 = 0x00;

/// AX=4400h: return BX's file/device information word in DX (and AX).
pub(super) fn ioctl(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    if call.cpu().al() != GET_DEVICE_INFORMATION {
        return return_result(call, Err(DosError::InvalidFunction));
    }
    let handle = call.cpu().bx();
    let result = call
        .state()
        .files
        .device_information(handle)
        .map(|information| {
            let mut cpu = call.cpu();
            cpu.set_dx(information);
            cpu.set_ax(information);
        });
    return_result(call, result)
}

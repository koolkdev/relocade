//! DOS device information, mode control and readiness for open handles.

use super::Services;
use crate::DosError;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_match},
    CpuContext,
};

#[interrupt(context(call), args(function = al))]
pub(super) async fn dispatch(call: &mut CpuContext<'_>, services: &Services<'_>, function: u8) {
    interrupt_match!(
        call,
        match function {
            0x00 => device_information(services),
            0x01 => set_device_information(services),
            0x06 => input_status(services),
            0x07 => output_status(services),
            _ => unsupported,
        }
    );
}

#[interrupt(args(handle = bx), returns {
    Ok(info) => { dx = info, ax = info, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
fn device_information(services: &Services<'_>, handle: u16) -> Result<u16, DosError> {
    services.process.files().device_information(handle)
}

#[interrupt(args(handle = bx, info = dx), returns {
    Ok(previous) => { al = previous, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
fn set_device_information(services: &Services<'_>, handle: u16, info: u16) -> Result<u8, DosError> {
    services
        .process
        .files()
        .set_device_information(handle, info)
}

#[interrupt(args(handle = bx), returns {
    Ok(status) => { al = status, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
async fn input_status(services: &Services<'_>, handle: u16) -> Result<u8, DosError> {
    let handle = services.process.files().handle(handle)?.clone();
    Ok(if handle.input_available().await? {
        0xff
    } else {
        0
    })
}

#[interrupt(args(handle = bx), returns {
    Ok(status) => { al = status, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
async fn output_status(services: &Services<'_>, handle: u16) -> Result<u8, DosError> {
    let handle = services.process.files().handle(handle)?.clone();
    Ok(if handle.output_ready().await? {
        0xff
    } else {
        0
    })
}

#[interrupt(returns { error => crate::interrupt::return_error(error) })]
fn unsupported() -> DosError {
    DosError::InvalidFunction
}

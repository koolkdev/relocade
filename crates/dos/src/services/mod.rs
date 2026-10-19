//! DOS interrupt dispatch and service handlers.

mod console;
mod directories;
mod files;
mod memory;

use crate::{DosState, ServiceError};
use std::ops::ControlFlow;
use wasm86_runtime::HostCall;

const WRITE_STRING: u8 = 0x09;
const SELECT_DRIVE: u8 = 0x0e;
const GET_DRIVE: u8 = 0x19;
const CHANGE_DIRECTORY: u8 = 0x3b;
const GET_DIRECTORY: u8 = 0x47;
const CREATE_HANDLE: u8 = 0x3c;
const OPEN_HANDLE: u8 = 0x3d;
const CLOSE_HANDLE: u8 = 0x3e;
const READ_HANDLE: u8 = 0x3f;
const WRITE_HANDLE: u8 = 0x40;
const SEEK_HANDLE: u8 = 0x42;
const ALLOCATE_MEMORY: u8 = 0x48;
const FREE_MEMORY: u8 = 0x49;
const RESIZE_MEMORY: u8 = 0x4a;
const TERMINATE_PROCESS: u8 = 0x4c;

/// INT 21h selects a DOS service through AH.
pub(crate) fn int21(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    match call.cpu().ah() {
        WRITE_STRING => console::write_string(call),
        SELECT_DRIVE => directories::select_drive(call),
        GET_DRIVE => directories::get_drive(call),
        CHANGE_DIRECTORY => directories::change_directory(call),
        GET_DIRECTORY => directories::get_directory(call),
        CREATE_HANDLE => files::create(call),
        OPEN_HANDLE => files::open(call),
        CLOSE_HANDLE => files::close(call),
        READ_HANDLE => files::read(call),
        WRITE_HANDLE => files::write(call),
        SEEK_HANDLE => files::seek(call),
        ALLOCATE_MEMORY => memory::allocate(call),
        FREE_MEMORY => memory::free(call),
        RESIZE_MEMORY => memory::resize(call),
        TERMINATE_PROCESS => terminate_process(call),
        function => return_to_host(call, Err(ServiceError::UnsupportedFunction(function))),
    }
}

/// AH=4Ch: terminate the current process with the exit code in AL.
fn terminate_process(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let code = call.cpu().al();
    return_to_host(call, Ok(code))
}

/// INT 20h terminates without an exit code, including a COM program's initial RET.
pub(crate) fn int20(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    return_to_host(call, Ok(0))
}

fn return_to_host(
    call: &mut HostCall<'_, DosState>,
    status: Result<u8, ServiceError>,
) -> ControlFlow<()> {
    if status.is_ok() {
        // The only process has exited; all of its conventional memory is free.
        crate::memory::reset(&mut call.memory());
        call.state_mut().files.close_all();
    }
    call.state_mut().exit_status = Some(status);
    ControlFlow::Break(())
}

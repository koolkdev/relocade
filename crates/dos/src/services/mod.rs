//! DOS interrupt dispatch and service handlers.

mod console;
mod directories;
mod files;
mod ioctl;
mod memory;
mod search;
mod system;

use crate::{files::CreateMode, DosState, ServiceError};
use relocade_runtime::HostCall;
use std::ops::ControlFlow;

const DIRECT_CONSOLE_IO: u8 = 0x06;
const WRITE_STRING: u8 = 0x09;
const SELECT_DRIVE: u8 = 0x0e;
const GET_DRIVE: u8 = 0x19;
const SET_VECTOR: u8 = 0x25;
const GET_VERSION: u8 = 0x30;
const GET_VECTOR: u8 = 0x35;
const CREATE_DIRECTORY: u8 = 0x39;
const REMOVE_DIRECTORY: u8 = 0x3a;
const CHANGE_DIRECTORY: u8 = 0x3b;
const GET_DIRECTORY: u8 = 0x47;
const SET_DTA: u8 = 0x1a;
const GET_DTA: u8 = 0x2f;
const FIND_FIRST: u8 = 0x4e;
const FIND_NEXT: u8 = 0x4f;
const CREATE_HANDLE: u8 = 0x3c;
const CREATE_TEMPORARY_HANDLE: u8 = 0x5a;
const CREATE_NEW_HANDLE: u8 = 0x5b;
const RENAME_FILE: u8 = 0x56;
const OPEN_HANDLE: u8 = 0x3d;
const CLOSE_HANDLE: u8 = 0x3e;
const COMMIT_HANDLE: u8 = 0x68;
const COMMIT_HANDLE_ALIAS: u8 = 0x6a;
const READ_HANDLE: u8 = 0x3f;
const WRITE_HANDLE: u8 = 0x40;
const DELETE_FILE: u8 = 0x41;
const SEEK_HANDLE: u8 = 0x42;
const FILE_ATTRIBUTES: u8 = 0x43;
const IOCTL: u8 = 0x44;
const DUPLICATE_HANDLE: u8 = 0x45;
const FORCE_DUPLICATE_HANDLE: u8 = 0x46;
const ALLOCATE_MEMORY: u8 = 0x48;
const FREE_MEMORY: u8 = 0x49;
const RESIZE_MEMORY: u8 = 0x4a;
const TERMINATE_PROCESS: u8 = 0x4c;
const LONG_FILENAME: u8 = 0x71;

/// INT 21h selects a DOS service through AH.
pub(crate) fn int21(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    match call.cpu().ah() {
        DIRECT_CONSOLE_IO => console::direct_io(call),
        WRITE_STRING => console::write_string(call),
        SELECT_DRIVE => directories::select_drive(call),
        GET_DRIVE => directories::get_drive(call),
        SET_VECTOR => system::set_vector(call),
        GET_VERSION => system::version(call),
        GET_VECTOR => system::get_vector(call),
        CREATE_DIRECTORY => directories::create_directory(call),
        REMOVE_DIRECTORY => directories::remove_directory(call),
        CHANGE_DIRECTORY => directories::change_directory(call),
        GET_DIRECTORY => directories::get_directory(call),
        SET_DTA => search::set_dta(call),
        GET_DTA => search::get_dta(call),
        FIND_FIRST => search::find_first(call),
        FIND_NEXT => search::find_next(call),
        CREATE_HANDLE => files::create(call, CreateMode::TruncateExisting),
        CREATE_TEMPORARY_HANDLE => files::create_temporary(call),
        CREATE_NEW_HANDLE => files::create(call, CreateMode::FailIfExists),
        RENAME_FILE => files::rename(call),
        OPEN_HANDLE => files::open(call),
        CLOSE_HANDLE => files::close(call),
        COMMIT_HANDLE | COMMIT_HANDLE_ALIAS => files::commit(call),
        READ_HANDLE => files::read(call),
        WRITE_HANDLE => files::write(call),
        DELETE_FILE => files::delete(call),
        SEEK_HANDLE => files::seek(call),
        FILE_ATTRIBUTES => files::attributes(call),
        IOCTL => ioctl::ioctl(call),
        DUPLICATE_HANDLE => files::duplicate(call),
        FORCE_DUPLICATE_HANDLE => files::force_duplicate(call),
        ALLOCATE_MEMORY => memory::allocate(call),
        FREE_MEMORY => memory::free(call),
        RESIZE_MEMORY => memory::resize(call),
        TERMINATE_PROCESS => terminate_process(call),
        LONG_FILENAME => files::long_filename(call),
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

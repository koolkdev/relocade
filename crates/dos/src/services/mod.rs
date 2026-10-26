//! DOS interrupt dispatch and service handlers.

mod buffers;
mod console;
mod directories;
mod files;
mod ioctl;
mod memory;
mod search;
mod system;

use crate::{files::CreateMode, state::DosState, DosServices, ServiceError};
use relocade_machine::{CpuContext, HookResult};
use relocade_x86::{ExecutionExit, HostExit};

const READ_CHARACTER_ECHO: u8 = 0x01;
const WRITE_CHARACTER: u8 = 0x02;
const DIRECT_CONSOLE_IO: u8 = 0x06;
const READ_CHARACTER: u8 = 0x07;
const READ_CHARACTER_CHECK_BREAK: u8 = 0x08;
const WRITE_STRING: u8 = 0x09;
const READ_BUFFERED: u8 = 0x0a;
const INPUT_STATUS: u8 = 0x0b;
const FLUSH_INPUT: u8 = 0x0c;
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
pub(crate) fn int21(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    match call.cpu.ah() {
        DIRECT_CONSOLE_IO => console::direct_io(call, state),
        READ_CHARACTER_ECHO | READ_CHARACTER | READ_CHARACTER_CHECK_BREAK => {
            console::read_character(call, state)
        }
        WRITE_CHARACTER => console::write_character(call, state),
        WRITE_STRING => console::write_string(call, state),
        READ_BUFFERED => console::read_buffered(call, state),
        INPUT_STATUS => console::input_status(call, state),
        FLUSH_INPUT => console::flush_input(call, state),
        SELECT_DRIVE => directories::select_drive(call, state),
        GET_DRIVE => directories::get_drive(call, state),
        SET_VECTOR => system::set_vector(call),
        GET_VERSION => system::version(call),
        GET_VECTOR => system::get_vector(call),
        CREATE_DIRECTORY => directories::create_directory(call, state),
        REMOVE_DIRECTORY => directories::remove_directory(call, state),
        CHANGE_DIRECTORY => directories::change_directory(call, state),
        GET_DIRECTORY => directories::get_directory(call, state),
        SET_DTA => search::set_dta(call, state),
        GET_DTA => search::get_dta(call, state),
        FIND_FIRST => search::find_first(call, state),
        FIND_NEXT => search::find_next(call, state),
        CREATE_HANDLE => files::create(call, state, CreateMode::TruncateExisting),
        CREATE_TEMPORARY_HANDLE => files::create_temporary(call, state),
        CREATE_NEW_HANDLE => files::create(call, state, CreateMode::FailIfExists),
        RENAME_FILE => files::rename(call, state),
        OPEN_HANDLE => files::open(call, state),
        CLOSE_HANDLE => files::close(call, state),
        COMMIT_HANDLE | COMMIT_HANDLE_ALIAS => files::commit(call, state),
        READ_HANDLE => files::read(call, state),
        WRITE_HANDLE => files::write(call, state),
        DELETE_FILE => files::delete(call, state),
        SEEK_HANDLE => files::seek(call, state),
        FILE_ATTRIBUTES => files::attributes(call, state),
        IOCTL => ioctl::ioctl(call, state),
        DUPLICATE_HANDLE => files::duplicate(call, state),
        FORCE_DUPLICATE_HANDLE => files::force_duplicate(call, state),
        ALLOCATE_MEMORY => memory::allocate(call),
        FREE_MEMORY => memory::free(call),
        RESIZE_MEMORY => memory::resize(call),
        TERMINATE_PROCESS => terminate_process(call, state),
        LONG_FILENAME => files::long_filename(call),
        function => return_to_host(
            call,
            state,
            Err(ServiceError::UnsupportedFunction(function)),
        ),
    }
}

/// AH=4Ch: terminate the current process with the exit code in AL.
fn terminate_process(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let code = call.cpu.al();
    return_to_host(call, state, Ok(code))
}

/// INT 20h terminates without an exit code, including a COM program's initial RET.
pub(crate) fn int20(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    return_to_host(call, state, Ok(0))
}

fn return_to_host(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    status: Result<u8, ServiceError>,
) -> HookResult {
    state.process.as_ref().expect("active DOS process").finish(
        &mut call.memory,
        &mut state.filesystem,
        status,
    );
    HookResult::Exit(ExecutionExit::Host(HostExit::Return))
}

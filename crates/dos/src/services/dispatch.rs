//! INT 21h selection; each service owns its argument and return mapping.

use super::{console, control, directories, files, ioctl, memory, search, system, Services};
use crate::ServiceError;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext,
};

#[interrupt(context(call))]
pub(crate) async fn int21(call: &mut CpuContext<'_>, services: &Services<'_>) {
    // Conventional DOS 5 ends at AH=6Ch. Unknown calls return before entering
    // DOS or checking BREAK; AH=71h retains its explicit LFN-unavailable result.
    interrupt_dispatch!(
        call,
        match call.cpu.ah() {
            0x51 | 0x62 => system::current_psp(services),
            0x00..=0x6c | 0x71 => dispatch_function(services),
            _ => null_function,
        }
    );
}

#[interrupt(context(call))]
async fn dispatch_function(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let mut request = control::Request::enter(call, services).await;
    interrupt_dispatch!(
        call,
        match request.function {
            0x01 => console::read_character_echo(&mut request),
            0x02 => console::write_character(&mut request),
            0x06 => console::direct_io(&mut request),
            0x07 => console::read_character(&mut request),
            0x08 => console::read_character_check_break(&mut request),
            0x09 => console::write_string(&mut request),
            0x0a => console::read_buffered(&mut request),
            0x0b => console::input_status(&mut request),
            0x0c => console::flush_input(&mut request),
            0x0e => directories::select_drive(services),
            0x18 | 0x1d | 0x1e | 0x20 | 0x61 | 0x6b => null_function,
            0x19 => directories::get_drive(services),
            0x1a => search::set_dta(services),
            0x25 => system::set_vector,
            0x2f => search::get_dta(services),
            0x30 => system::version,
            0x33 => control::break_setting(services),
            0x34 => system::indos,
            0x35 => system::get_vector,
            0x39 => directories::create_directory(services),
            0x3a => directories::remove_directory(services),
            0x3b => directories::change_directory(services),
            0x3c => files::create_or_truncate(services),
            0x3d => files::open(services),
            0x3e => files::close(services),
            0x3f => files::io::read(&mut request),
            0x40 => files::io::write(&mut request),
            0x41 => files::metadata::delete(services),
            0x42 => files::seek(services),
            0x43 => files::metadata::attributes(services),
            0x44 => ioctl::dispatch(services),
            0x45 => files::duplicate(services),
            0x46 => files::force_duplicate(services),
            0x47 => directories::get_directory(services),
            0x48 => memory::allocate(services),
            0x49 => memory::free,
            0x4a => memory::resize,
            0x4c => terminate_process(services),
            0x4e => search::find_first(services),
            0x4f => search::find_next(services),
            0x56 => files::metadata::rename(services),
            0x5a => files::create_temporary(services),
            0x5b => files::create_new(services),
            0x68 => files::commit(services),
            0x6a => files::commit_alias(services),
            0x71 => files::long_filename,
            _ => unsupported(services),
        }
    );
}

#[interrupt(returns { () => { al = 0u8 } })]
fn null_function() {}

#[interrupt(context(call), args(code = al))]
fn terminate_process(call: &mut CpuContext<'_>, services: &Services<'_>, code: u8) {
    services.exit(&mut call.memory, Ok(code));
}

#[interrupt(context(call), args(function = ah))]
fn unsupported(call: &mut CpuContext<'_>, services: &Services<'_>, function: u8) {
    services.exit(
        &mut call.memory,
        Err(ServiceError::UnsupportedFunction(function)),
    );
}

/// INT 20h terminates without an exit code, including a COM program's initial RET.
pub(crate) async fn int20(call: &mut CpuContext<'_>, services: &Services<'_>) {
    services.exit(&mut call.memory, Ok(0));
}

//! INT 21h selection; each service owns its argument and return mapping.

use super::{console, directories, files, ioctl, memory, search, system, Services};
use crate::ServiceError;
use console::Input;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, MemoryView,
};

interrupt_dispatch! {
    pub(crate) int21(ah, services: &Services<'_>) {
        0x01: console::read_character_echo(Input::new(services)),
        0x02: console::write_character(services),
        0x06: console::direct_io(Input::new(services)),
        0x07: console::read_character(Input::new(services)),
        0x08: console::read_character_check_break(Input::new(services)),
        0x09: console::write_string(services),
        0x0a: console::read_buffered(Input::new(services)),
        0x0b: console::input_status(services),
        0x0c: console::flush_input(services),
        0x0e: directories::select_drive(services),
        0x19: directories::get_drive(services),
        0x1a: search::set_dta(services),
        0x25: system::set_vector,
        0x2f: search::get_dta(services),
        0x30: system::version,
        0x35: system::get_vector,
        0x39: directories::create_directory(services),
        0x3a: directories::remove_directory(services),
        0x3b: directories::change_directory(services),
        0x3c: files::create_or_truncate(services),
        0x3d: files::open(services),
        0x3e: files::close(services),
        0x3f: files::io::read(services),
        0x40: files::io::write(services),
        0x41: files::metadata::delete(services),
        0x42: files::seek(services),
        0x43: files::metadata::attributes(services),
        0x44: ioctl::dispatch(services),
        0x45: files::duplicate(services),
        0x46: files::force_duplicate(services),
        0x47: directories::get_directory(services),
        0x48: memory::allocate(services),
        0x49: memory::free,
        0x4a: memory::resize,
        0x4c: terminate_process(services),
        0x4e: search::find_first(services),
        0x4f: search::find_next(services),
        0x56: files::metadata::rename(services),
        0x5a: files::create_temporary(services),
        0x5b: files::create_new(services),
        0x68: files::commit(services),
        0x6a: files::commit_alias(services),
        0x71: files::long_filename,
        _: unsupported(services),
    }
}

#[interrupt(context(memory), args(code = al))]
fn terminate_process(memory: &mut MemoryView<'_>, services: &Services<'_>, code: u8) {
    services.exit(memory, Ok(code));
}

#[interrupt(context(memory), args(function = ah))]
fn unsupported(memory: &mut MemoryView<'_>, services: &Services<'_>, function: u8) {
    services.exit(memory, Err(ServiceError::UnsupportedFunction(function)));
}

/// INT 20h terminates without an exit code, including a COM program's initial RET.
pub(crate) async fn int20(call: &mut CpuContext<'_>, services: &Services<'_>) {
    services.exit(&mut call.memory, Ok(0));
}

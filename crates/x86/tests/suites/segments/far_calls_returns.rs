//! Protected far CALL and RET validate frames before committing CS and ESP.

#[path = "far_calls_returns/continuation.rs"]
mod continuation;
#[path = "far_calls_returns/faults.rs"]
mod faults;
#[path = "far_calls_returns/fetch.rs"]
mod fetch;
#[path = "far_calls_returns/frames.rs"]
mod frames;
#[path = "far_calls_returns/sources.rs"]
mod sources;

use super::{
    data,
    selector_cases::{
        check_one, code_defaults, code_descriptor as descriptor, far_pointer as pointer, loaded,
    },
};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    DescriptorTables, PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentProfile,
};

fn immediate(word: bool, offset: u32, selector: u16) -> Vec<u8> {
    let mut code = if word { vec![0x66, 0x9a] } else { vec![0x9a] };
    code.extend(pointer(word, offset, selector));
    code
}

fn ret(word: bool, cleanup: Option<u16>) -> Vec<u8> {
    let mut code = if word { vec![0x66] } else { vec![] };
    code.push(if cleanup.is_some() { 0xca } else { 0xcb });
    if let Some(cleanup) = cleanup {
        code.extend(cleanup.to_le_bytes());
    }
    code
}

fn tables(limit: u32) -> DescriptorTables {
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0xc000, limit, SegmentDefaultSize::Bits32));
    tables
}

fn image_with_stack(code: &[u8], esp: u32) -> Image {
    let mut image = Image::new(code);
    image.cpu.segments.cs.selector = 0x1b;
    image.cpu.registers.esp = esp;
    image.map(9, 0x8000, true);
    image.data(0x8000, &[0xa5; 32]);
    image
}

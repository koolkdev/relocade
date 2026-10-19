//! Packed decimal loads and stores retain eighteen digits and signed zero.

#[path = "x87_bcd/load.rs"]
mod load;
#[path = "x87_bcd/store.rs"]
mod store;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    x87::{complete_x87, dispatch, real80, set_control, stack_image, status, write_register_bits},
};
use wasm86_x86::{CpuState, SegmentProfile};

const MAXIMUM: [u8; 10] = [0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0];

fn image(code: &[u8], top: u8, tags: u16) -> Image {
    let mut image = stack_image(code, top, tags);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, true);
    image
}

fn completed(cpu: CpuState, bytes: u32, opcode: u16, address: u32) -> CpuState {
    let mut cpu = complete_x87(cpu, bytes, opcode);
    cpu.x87.data_offset = address;
    cpu.x87.data_selector = 0x23;
    cpu
}

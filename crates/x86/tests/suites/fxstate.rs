//! FXSR images combine physical tag bits, logical x87 payloads and literal SIMD state.

#[path = "fxstate/mmx_xmm.rs"]
mod mmx_xmm;
#[path = "fxstate/restore.rs"]
mod restore;
#[path = "fxstate/save.rs"]
mod save;
#[path = "fxstate/transfers.rs"]
mod transfers;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    x87::{complete_x87, dispatch, real80, set_control, stack_image, status, write_register_bits},
};
use wasm86_x86::{CpuState, ExecutionProfile, SegmentProfile, Segments};

const FLAT32: ExecutionProfile = ExecutionProfile::Protected(SegmentProfile::Flat32);
const PROFILES: [ExecutionProfile; 3] = [
    FLAT32,
    ExecutionProfile::Protected(SegmentProfile::Segmented16),
    ExecutionProfile::Real16,
];

fn sixteen_bit(profile: ExecutionProfile) -> bool {
    matches!(
        profile,
        ExecutionProfile::Real16 | ExecutionProfile::Protected(SegmentProfile::Segmented16)
    )
}

fn instruction(profile: ExecutionProfile, restore: bool, address: u32) -> Vec<u8> {
    let mut code = vec![0x0f, 0xae];
    code.push((u8::from(restore) << 3) | if sixteen_bit(profile) { 6 } else { 5 });
    let address = address.to_le_bytes();
    code.extend_from_slice(&address[..if sixteen_bit(profile) { 2 } else { 4 }]);
    code
}

fn image(code: &[u8], profile: ExecutionProfile) -> Image {
    let mut image = stack_image(code, 5, 0x3f95);
    image.cpu.x87.status = status(0x2800);
    image.cpu.simd.mxcsr = 0xabcd_9fc5;
    for (index, xmm) in image.cpu.simd.xmm.iter_mut().enumerate() {
        *xmm = std::array::from_fn(|byte| index as u8 * 17 + byte as u8);
    }
    match profile {
        ExecutionProfile::Real16 => image.cpu.segments = Segments::real_mode(),
        ExecutionProfile::Protected(SegmentProfile::Segmented16) => {
            image.cpu.segments.cs.big = 0;
        }
        _ => {}
    }
    image.map(4, 0x8000, true);
    image.data(0x8010, &[0xa5; 512]);
    image
}

fn retire(mut cpu: CpuState, bytes: usize) -> CpuState {
    cpu.eip += bytes as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn saved_bytes(cpu: &CpuState, header: [u32; 8], order: [usize; 8]) -> [u8; 512] {
    let mut bytes = [0xa5; 512];
    for (index, word) in header.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (logical, physical) in order.into_iter().enumerate() {
        let register = cpu.x87.registers[physical];
        let offset = 32 + logical * 16;
        bytes[offset..offset + 10]
            .copy_from_slice(&real80((register.significand, register.sign_exponent)));
    }
    for (index, xmm) in cpu.simd.xmm.iter().enumerate() {
        bytes[160 + index * 16..176 + index * 16].copy_from_slice(xmm);
    }
    bytes
}

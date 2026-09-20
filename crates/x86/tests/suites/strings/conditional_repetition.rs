//! REPE/REPNE stop on the last comparison and retain entry flags on faults.

#[path = "conditional_repetition/decoding.rs"]
mod decoding;
#[path = "conditional_repetition/faults.rs"]
mod faults;
#[path = "conditional_repetition/restart.rs"]
mod restart;
#[path = "conditional_repetition/values.rs"]
mod values;

use super::{flags, record, Operation};
use crate::support::cases::InstructionCase as Case;
use wasm86_x86::{Segment, SegmentAttributes, StoredSegment};

const COMPARISONS: [Operation; 2] = [Operation::Cmps, Operation::Scas];

fn code(operation: Operation, prefix: u8, width: u32, address16: bool, default16: bool) -> Vec<u8> {
    let mut code = vec![prefix];
    if width != 1 && (width == 2) != default16 {
        code.push(0x66);
    }
    if address16 != default16 {
        code.push(0x67);
    }
    let opcode = match operation {
        Operation::Cmps => 0xa6,
        Operation::Scas => 0xae,
        _ => unreachable!(),
    };
    code.push(opcode + u8::from(width != 1));
    code
}

fn profile(case: Case, default16: bool) -> Case {
    if default16 {
        case.segmented_only().segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        )
    } else {
        case
    }
}

fn bytes(values: &[u32], width: u32, backward: bool) -> Vec<u8> {
    let mut values = values.to_vec();
    if backward {
        values.reverse();
    }
    values
        .iter()
        .flat_map(|value| value.to_le_bytes()[..width as usize].to_vec())
        .collect()
}

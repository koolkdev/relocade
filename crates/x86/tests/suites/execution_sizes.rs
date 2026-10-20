//! CS.D, operand/address overrides and SS.B have independent responsibilities.

#[path = "execution_sizes/addressing.rs"]
mod addressing;
#[path = "execution_sizes/branches.rs"]
mod branches;
#[path = "execution_sizes/decoding.rs"]
mod decoding;
#[path = "execution_sizes/stack.rs"]
mod stack;
#[path = "execution_sizes/strings.rs"]
mod strings;

use crate::support::cases::InstructionCase as Case;
use relocade_x86::{Segment, SegmentRegister};

fn code16(case: Case) -> Case {
    case.segmented_only().segment(
        Segment::Cs,
        SegmentRegister {
            big: 0,
            ..SegmentRegister::flat_code32(0x1b)
        },
    )
}

fn stack16(limit: u32) -> SegmentRegister {
    SegmentRegister {
        limit,
        big: 0,
        ..SegmentRegister::flat_data32(0x23)
    }
}

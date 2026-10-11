//! MMX packing concatenates the two 64-bit sources before narrowing them.

use super::*;
use wasm86_compiler::SaturatingLane;

instruction_families! {
    PACKSSWB {
        execute: binary::<_>(|left, right| pack::<I8>(left, right, true));
        forms { NP 0x0f 0x63 => qword(mm, mm_rm); }
    }
    PACKSSDW {
        execute: binary::<_>(|left, right| pack::<I16>(left, right, true));
        forms { NP 0x0f 0x6b => qword(mm, mm_rm); }
    }
    PACKUSWB {
        execute: binary::<_>(|left, right| pack::<I8>(left, right, false));
        forms { NP 0x0f 0x67 => qword(mm, mm_rm); }
    }
}

fn pack<T: SaturatingLane>(left: Val<I64>, right: Val<I64>, signed: bool) -> Val<I64> {
    let input = Val::<V128>::from(0_u128)
        .replace_lane(0, left)
        .replace_lane(1, right);
    let result = if signed {
        input.signed_narrow_lanes::<T>(0_u128)
    } else {
        // PACKUSWB still interprets its word inputs as signed.
        input.unsigned_narrow_lanes::<T>(0_u128)
    };
    result.extract_lane(0)
}

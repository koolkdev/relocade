use std::panic::{catch_unwind, AssertUnwindSafe};
use wasm86_x86::{CpuState, SegmentAttributes, SegmentProfile};

use super::BlockModules;

#[test]
fn cache_hits_revalidate_cs_even_when_the_profile_stays_compatible() {
    let profile = SegmentProfile::Segmented32;
    let mut cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    cpu.segments.cs.limit = 0x1001;
    let mut blocks = BlockModules::default();
    blocks.get(&cpu, &[0xb0, 7], 1, profile);
    let valid = cpu;
    for cs in [
        wasm86_x86::StoredSegment {
            limit: 0x1000,
            ..valid.segments.cs
        },
        wasm86_x86::StoredSegment {
            attributes: SegmentAttributes::from_bits(0x10),
            ..valid.segments.cs
        },
    ] {
        cpu.segments.cs = cs;
        assert!(profile.is_compatible_with(&cpu.segments));
        assert!(catch_unwind(AssertUnwindSafe(|| {
            blocks.get(&cpu, &[0xb0, 7], 1, profile);
        }))
        .is_err());
    }
    blocks.get(&valid, &[0xb0, 7], 1, profile);
}

#[test]
fn admission_checks_only_instructions_inside_the_compilation_boundary() {
    let mut cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    let mut blocks = BlockModules::default();
    for (bytes, limit, cs_limit) in [
        (&[0x90, 0x0f][..], 1, 0x1000),
        (&[0xeb, 0, 0x0f][..], 3, 0x1001),
        (&[0xf3, 0xa4, 0x0f][..], 3, 0x1001),
    ] {
        cpu.segments.cs.limit = cs_limit;
        blocks.get(&cpu, bytes, limit, SegmentProfile::Segmented32);
    }
}

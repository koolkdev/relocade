mod blocks;
mod budget_shape;
mod checkpoints;
mod dispatch;
mod fetch;
mod memory;

use super::*;
use crate::SegmentProfile;

#[test]
fn snapshot_helpers_use_unobserved_configuration() {
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        let code = [0xd8, 0xc9];
        let ordinary = compile_block_from_bytes_with_profile(0x1000, &code, 1, profile).unwrap();
        let configured = Compiler::new(profile)
            .compile_block(0x1000, &code, 1)
            .unwrap();
        assert!(ordinary.bytes == configured.bytes, "{profile:?}");
        assert_eq!(ordinary.requirements, configured.requirements);
        assert_eq!(
            configured.requirements.baseline_profile(),
            Some(profile.into())
        );
    }
}

#[test]
fn mxcsr_independent_instructions_do_not_guard_observations() {
    let mut observed = CpuState::default();
    observed.simd.mxcsr = 0xffff;
    for code in [
        &[0xf2, 0x0f, 0x2a, 0xc1][..], // CVTSI2SD
        &[0xf3, 0x0f, 0xe6, 0xc1],     // CVTDQ2PD
        &[0x66, 0x0f, 0x2a, 0xc1],     // CVTPI2PD register
        &[0x66, 0x0f, 0x2a, 0x00],     // CVTPI2PD memory
        &[0xf3, 0x0f, 0x53, 0xc1],     // RCPSS
        &[0x0f, 0x53, 0xc1],           // RCPPS
        &[0xf3, 0x0f, 0x52, 0xc1],     // RSQRTSS
        &[0x0f, 0x52, 0xc1],           // RSQRTPS
        &[0x0f, 0xae, 0x18],           // STMXCSR
        &[0x0f, 0xae, 0x00],           // FXSAVE
    ] {
        let generic = Compiler::new(SegmentProfile::Flat32)
            .compile_block(0x1000, code, 1)
            .unwrap();
        let specialized = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, code, 1)
            .unwrap();
        assert!(generic.bytes == specialized.bytes, "{code:02x?}");
    }
}

#[test]
fn x87_only_observations_preserve_generic_simd_guards() {
    let code = [0xf2, 0x0f, 0x58, 0xc1]; // ADDSD XMM0,XMM1
    let ordinary = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 1)
        .unwrap();
    let partial = Compiler::new(SegmentProfile::Flat32)
        .with_observations(CpuObservations {
            x87: X87Observations {
                rounding_control: Some(2),
                ..Default::default()
            },
            ..Default::default()
        })
        .compile_block(0x1000, &code, 1)
        .unwrap();
    assert_eq!(ordinary.bytes, partial.bytes);
    assert_eq!(ordinary.requirements, partial.requirements);
}

#[test]
fn privilege_requirements_preserve_policy_and_record_only_consumers() {
    use crate::{EntryFact, PrivilegeLevel};
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let cpl = EntryFact::PrivilegeLevel(PrivilegeLevel::Ring3);
    let tsd = EntryFact::Cr4 {
        mask: 1 << 2,
        value: 0,
    };
    let mut cpu = CpuState::default();
    cpu.flags.bytes.iopl = 3;
    cpu.flags.bytes.if_ = 0;
    for (code, consumes_cpl, consumes_tsd) in [
        (&[0x90][..], false, false),      // NOP
        (&[0xfa][..], true, false),       // CLI still faults under effective IOPL0.
        (&[0xf4][..], true, false),       // HLT
        (&[0x9c][..], true, false),       // PUSHFD keeps the protected flag policy.
        (&[0x9d][..], true, false),       // POPFD uses the same policy for writes.
        (&[0x0f, 0x31][..], false, true), // RDTSC relies on TSD=0.
    ] {
        let block = compiler.compile_block(0x1000, code, 1).unwrap();
        assert_eq!(block.requirements.facts().contains(&cpl), consumes_cpl);
        assert_eq!(block.requirements.facts().contains(&tsd), consumes_tsd);
        assert!(block.requirements.matches(&cpu));
    }
}

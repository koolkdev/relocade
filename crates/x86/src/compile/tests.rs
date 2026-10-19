mod branch_hints;
mod budget;
mod dispatch;
mod fetch;

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
    }
}

#[test]
fn interpreter_entries_keep_the_profile_and_ignore_cpu_observations() {
    let mut observed = CpuState::default();
    observed.x87.control.precision_control = 2;
    observed.x87.control.rounding_control = 3;
    observed.x87.control.precision_mask = 1;
    observed.x87.status.precision = 1;
    observed.simd.mxcsr = 0x9fc5;
    let profile = SegmentProfile::Segmented16;
    let compiler = Compiler::new(profile).specialize_on_cpu(&observed);
    for (name, ordinary, configured) in [
        (
            "run",
            compile_interpreter(profile).unwrap(),
            compiler.compile_interpreter().unwrap(),
        ),
        (
            "step",
            compile_interpreter_step(profile).unwrap(),
            compiler.compile_interpreter_step().unwrap(),
        ),
    ] {
        assert_eq!(configured.entry, name);
        assert_eq!(configured.execution_target, Some(compiler.target()));
        assert!(ordinary.bytes == configured.bytes, "{name}");
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

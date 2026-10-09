use super::*;
use crate::SegmentProfile;

mod code_tracking;
mod slices;

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
        assert_eq!(configured.execution_profile, Some(profile.into()));
        assert!(ordinary.bytes == configured.bytes, "{name}");
    }
}

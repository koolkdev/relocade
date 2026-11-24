mod blocks;
mod budget_shape;
mod checkpoints;
mod dispatch;
mod fetch;
mod linked_blocks;
mod memory;

use super::*;
use crate::support::config::FLAT32;

#[test]
fn snapshot_helper_uses_the_flat32_configuration_without_observations() {
    let code = [0xd8, 0xc9];
    let ordinary = compile_block_from_bytes(0x1000, &code, 1).unwrap();
    let configured = Compiler::new(FLAT32)
        .compile_block(0x1000, &code, 1)
        .unwrap();
    assert!(ordinary.bytes == configured.bytes);
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
        let generic = Compiler::new(FLAT32)
            .compile_block(0x1000, code, 1)
            .unwrap();
        let specialized = Compiler::new(FLAT32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, code, 1)
            .unwrap();
        assert!(generic.bytes == specialized.bytes, "{code:02x?}");
    }
}

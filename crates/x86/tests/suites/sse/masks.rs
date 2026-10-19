use super::*;

fn masks(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (prefix, opcode, bits, expected) in [
        (
            &[0x66][..],
            0xd7,
            0x807f01ff_00fe7e81_aa558000_ff007f80_u128,
            0x95a9,
        ),
        (&[][..], 0x50, 0x80000000_7fffffff_ffffffff_00000080, 0b1010),
        (&[0x66][..], 0x50, 0x7fffffffffffffff_8000000000000000, 0b01),
    ] {
        let code = encoding(prefix, opcode, 0xc7); // eax, xmm7
        let mut image = image(&code);
        image.cpu.simd.xmm[7] = bits.to_le_bytes();
        image.cpu.registers.eax = u32::MAX;
        image.cpu.x87.status.error_summary = 1;
        let mut cpu = retire(image.cpu, code.len());
        cpu.registers.eax = expected;
        checks.check(
            "mask zero-extends and leaves SIMD state untouched",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

#[test]
fn sse_masks_reject_memory_sources() {
    use crate::support::step::TestModule;
    use relocade_x86::{compile_block_from_bytes, BlockError};
    for code in [
        &[0x0f, 0x50, 0x04][..],
        &[0x66, 0x0f, 0x50, 0x04],
        &[0x66, 0x0f, 0xd7, 0x04],
        &[0x0f, 0xd7, 0x04],
    ] {
        let address = 0x2000 - code.len() as u32;
        assert!(matches!(
            compile_block_from_bytes(address, code, 1),
            Err(BlockError::UnsupportedInstruction { .. })
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = address;
        image.data(address + 0x2000, code);
        image.check_unchanged_exit(
            TestModule::interpreter(),
            "register-only mask is rejected before fetching a missing SIB byte",
            Exit::Other(0x0008_000f_0000_0000 | u64::from(address)),
        );
    }
}

test_frontends!(sse_sign_masks, masks);

use super::*;
use crate::support::config::REAL16;
use crate::{BlockError, CpuFeature, CpuModel};

#[test]
fn a_block_requires_matching_offsets_and_one_decode_width() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let nop = |eip, width| SnapshotInstruction::decode(&[0x90], eip, width).unwrap();
    assert_eq!(
        DecodedBlock::new(CsRelative(0x1000), nop(0x1001, Bits16), true).err(),
        Some(BlockError::NoncontiguousInstruction {
            expected: 0x1000,
            actual: 0x1001
        }),
    );
    let mut block = DecodedBlock::new(CsRelative(0x1000), nop(0x1000, Bits16), true).unwrap();
    assert_eq!(
        block.push(nop(0x1002, Bits16)),
        Err(BlockError::NoncontiguousInstruction {
            expected: 0x1001,
            actual: 0x1002
        }),
    );
    assert_eq!(
        block.push(nop(0x1001, Bits32)),
        Err(BlockError::InconsistentCodeSize { address: 0x1001 }),
    );
    block.push(nop(0x1001, Bits16)).unwrap();
    assert_eq!(compile(vec![block]).spans, [(CsRelative(0x1000), 2)]);
}

#[test]
fn an_interior_fault_depends_on_the_selected_cpu_model() {
    let plan = || vec![block(CsRelative(0x1000), true, &[0x0f, 0x44, 0xc1, 0x90])]; // CMOVE AX,CX; NOP
    let compiler = Compiler::from_config(REAL16);
    assert_eq!(
        compiler.compile_decoded_blocks(plan()).unwrap().spans,
        [(CsRelative(0x1000), 4)]
    );
    let no_cmov =
        compiler.with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false));
    assert_eq!(
        no_cmov.compile_decoded_blocks(plan()).err(),
        Some(BlockError::InteriorBlockBoundary { address: 0x1000 }),
    );
}

#[test]
fn each_view_has_unique_starts_and_each_export_has_one_width() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let address = CsRelative(0x1000);
    let nop = |width, exported| {
        let instruction = SnapshotInstruction::decode(&[0x90], address.eip(), width).unwrap();
        DecodedBlock::new(address, instruction, exported).unwrap()
    };
    let compiler = Compiler::from_config(REAL16);
    for blocks in [
        vec![nop(Bits16, true), nop(Bits16, false)],
        vec![nop(Bits16, true), nop(Bits32, true)],
    ] {
        assert_eq!(
            compiler.compile_decoded_blocks(blocks).err(),
            Some(BlockError::DuplicateBlock { address }),
        );
    }
    let code = compiler
        .compile_decoded_blocks(vec![nop(Bits16, true), nop(Bits32, false)])
        .unwrap();
    assert_eq!(code.entries.len(), 1);
    assert_eq!(code.entries[0].code_default_size, Bits16);
    assert_eq!(code.spans, [(address, 1), (address, 1)]);
}

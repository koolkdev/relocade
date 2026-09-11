use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Ebx, Ecx, Esp};

#[rustfmt::skip]
fn entry_stack_address_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, target, stack, returned) in [
        (&[0xff, 0x14, 0x24][..], 0x1234_5678, 0x5000, &[3, 0x10, 0, 0][..]),
        (&[0xff, 0x54, 0x24, 0xfc], 0xd4c3_b2a1, 0x5000, &[4, 0x10, 0, 0]),
        (&[0xff, 0x54, 0x8c, 0xf4], 0x1234_5678, 0x5000, &[4, 0x10, 0, 0]),
        (&[0x66, 0xff, 0x14, 0x24], 0x5678, 0x5002, &[4, 0x10]),
        (&[0x66, 0xff, 0x54, 0x24, 0xfe], 0xd4c3, 0x5002, &[5, 0x10]),
        (&[0x66, 0xff, 0x54, 0x8c, 0xf4], 0x5678, 0x5002, &[5, 0x10]),
    ] {
        cases.push(Case::preserving_flags(format!("CALL uses entry ESP and reads target before overlapping push: {code:02x?}"), code)
            .register(Esp, 0x5004, stack).initial_register(Ecx, 3).dispatch(target)
            .memory(0x5000, &[0xa1, 0xb2, 0xc3, 0xd4, 0x78, 0x56, 0x34, 0x12, 0x5a], ReadWrite)
            .expect_memory(stack, returned));
    }
    for (code, target) in [(&[0xff, 0x24, 0x24][..], 0x1234_5678), (&[0x66, 0xff, 0x24, 0x24], 0x5678)] {
        cases.push(Case::preserving_flags(format!("JMP reads through ESP without changing the stack: {code:02x?}"), code)
            .initial_register(Esp, 0x5004).dispatch(target)
            .memory(0x5004, &[0x78, 0x56, 0x34, 0x12], ReadOnly));
    }
    cases
}

#[rustfmt::skip]
fn split_and_aliased_ranges() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack, target, returned) in [
        (&[0xff, 0x13][..], 0x9004, 0x1234_5678, &[2, 0x10, 0, 0][..]),
        (&[0x66, 0xff, 0x13], 0x9002, 0x5678, &[3, 0x10]),
    ] {
        cases.push(Case::preserving_flags(format!("CALL source and return slot alias one physical frame: {code:02x?}"), code)
            .register(Esp, stack, 0x9000).initial_register(Ebx, 0x6000).dispatch(target)
            .map_page(6, 0xb000, ReadWrite).map_page(9, 0xb000, ReadWrite)
            .backing(0xb000, &[0x78, 0x56, 0x34, 0x12, 0xa5]).expect_memory(0x9000, returned));
    }
    for (code, stack, target, returned) in [
        (&[0xff, 0x13][..], 0x5003, 0xf123_8001, &[2, 0x10, 0, 0][..]),
        (&[0x66, 0xff, 0x13], 0x5001, 0x8001, &[3, 0x10]),
    ] {
        cases.push(Case::preserving_flags(format!("CALL splits both source and return slot over noncontiguous frames: {code:02x?}"), code)
            .register(Esp, stack, 0x4fff).initial_register(Ebx, 0x6fff).dispatch(target)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .map_page(6, 0xb000, ReadOnly).map_page(7, 0xe000, ReadOnly)
            .memory(0x4ffe, &[0xa5; 6], ReadWrite).memory(0x6ffe, &[0x5a, 1, 0x80, 0x23, 0xf1, 0x5a], ReadOnly)
            .expect_memory(0x4fff, returned));
    }
    for (code, target) in [(&[0xff, 0x23][..], 0xf123_8001), (&[0x66, 0xff, 0x23], 0x8001)] {
        cases.push(Case::preserving_flags(format!("JMP reads a split noncontiguous target: {code:02x?}"), code)
            .initial_register(Ebx, 0x6fff).dispatch(target)
            .map_page(6, 0xb000, ReadOnly).map_page(7, 0xe000, ReadOnly)
            .memory(0x6ffe, &[0x5a, 1, 0x80, 0x23, 0xf1, 0x5a], ReadOnly));
    }
    for (code, stack, target) in [(&[0xc3][..], 0x5003, 0xf123_8001), (&[0x66, 0xc3], 0x5001, 0x8001)] {
        cases.push(Case::preserving_flags(format!("RET reads a split noncontiguous return address: {code:02x?}"), code)
            .register(Esp, 0x4fff, stack).dispatch(target)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4ffe, &[0x5a, 1, 0x80, 0x23, 0xf1, 0x5a], ReadOnly));
    }
    cases
}

#[rustfmt::skip]
fn stack_address_and_wrap_cases() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags("relative CALL wraps full ESP before a fitting dword push", &[0xe8, 0, 0, 0, 0])
            .register(Esp, 0, 0xffff_fffc).dispatch(0x1005)
            .memory(0xffff_fffc, &[0xa5; 4], ReadWrite).expect_memory(0xffff_fffc, &[5, 0x10, 0, 0]),
        Case::preserving_flags("word CALL wraps full ESP before a fitting word push", &[0x66, 0xe8, 0, 0])
            .register(Esp, 0, 0xffff_fffe).dispatch(0x1004)
            .memory(0xffff_fffe, &[0xa5; 2], ReadWrite).expect_memory(0xffff_fffe, &[4, 0x10]),
        Case::preserving_flags("CALL ESP saves the old zero target before stack wrap", &[0xff, 0xd4])
            .register(Esp, 0, 0xffff_fffc).dispatch(0)
            .memory(0xffff_fffc, &[0xa5; 4], ReadWrite).expect_memory(0xffff_fffc, &[2, 0x10, 0, 0]),
        Case::preserving_flags("CALL SP saves the old zero target before stack wrap", &[0x66, 0xff, 0xd4])
            .register(Esp, 0, 0xffff_fffe).dispatch(0)
            .memory(0xffff_fffe, &[0xa5; 2], ReadWrite).expect_memory(0xffff_fffe, &[3, 0x10]),
        Case::preserving_flags("RET wraps full ESP after a fitting dword pop", &[0xc3])
            .register(Esp, 0xffff_fffc, 0).dispatch(0xfedc_ba98).memory(0xffff_fffc, &[0x98, 0xba, 0xdc, 0xfe], ReadOnly),
        Case::preserving_flags("word RET wraps full ESP after a fitting word pop", &[0x66, 0xc3])
            .register(Esp, 0xffff_fffe, 0).dispatch(0x8001).memory(0xffff_fffe, &[1, 0x80], ReadOnly),
        Case::preserving_flags("RET cleanup follows the wrapped dword pop", &[0xc2, 0xff, 0xff])
            .register(Esp, 0xffff_fffc, 0xffff).dispatch(0xfedc_ba98).memory(0xffff_fffc, &[0x98, 0xba, 0xdc, 0xfe], ReadOnly),
        Case::preserving_flags("word RET cleanup follows the wrapped word pop", &[0x66, 0xc2, 0xff, 0xff])
            .register(Esp, 0xffff_fffe, 0xffff).dispatch(0x8001).memory(0xffff_fffe, &[1, 0x80], ReadOnly),
        Case::preserving_flags("word CALL borrows through the full ESP high half", &[0x66, 0xe8, 0, 0])
            .register(Esp, 0x1234_0000, 0x1233_fffe).dispatch(0x1004)
            .memory(0x1233_fffe, &[0xa5; 2], ReadWrite).expect_memory(0x1233_fffe, &[4, 0x10]),
        Case::preserving_flags("word RET carries through the full ESP high half", &[0x66, 0xc3])
            .register(Esp, 0x1234_fffe, 0x1235_0000).dispatch(0xbeef).memory(0x1234_fffe, &[0xef, 0xbe], ReadOnly),
        Case::preserving_flags("word RET cleanup does not truncate full ESP", &[0x66, 0xc2, 0xff, 0xff])
            .register(Esp, 0x1234_fffe, 0x1235_ffff).dispatch(0xbeef).memory(0x1234_fffe, &[0xef, 0xbe], ReadOnly),
    ];
    for (code, stack, returned) in [(&[0x66, 0xff, 0x13][..], 0x1234_9002, &[3, 0x10][..]), (&[0x66, 0xff, 0x23], 0x1234_9004, &[][..])] {
        let mut case = Case::preserving_flags(format!("word indirect control keeps a dword source address: {code:02x?}"), code)
            .initial_register(Ebx, 0x1234_6000).register(Esp, 0x1234_9004, stack).dispatch(0x8001)
            .memory(0x1234_6000, &[1, 0x80], ReadOnly).memory(0x1234_9000, &[0xa5; 6], ReadWrite);
        if !returned.is_empty() { case = case.expect_memory(stack, returned); }
        cases.push(case);
    }
    cases
}

test_cases!(
    entry_esp_and_overlapping_sources,
    entry_stack_address_cases()
);
test_cases!(
    split_and_physically_aliased_targets,
    split_and_aliased_ranges()
);
test_cases!(
    full_stack_address_width_and_wrap,
    stack_address_and_wrap_cases()
);

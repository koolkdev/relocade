//! LEA computes an address without accessing data or changing flags.
use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly},
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::*};
use wasmparser::{Parser, Payload, TypeRef};

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("LEA EDI,[disp32]: absolute displacement", &[0x8d, 0x3d, 0x78, 0x56, 0x34, 0x12])
            .register(Edi, 0x8888_8888, 0x1234_5678),
        Case::preserving_flags("LEA ESI,[disp32]: SIB without base or index", &[0x8d, 0x34, 0x25, 0x98, 0xba, 0xdc, 0xfe])
            .register(Esi, 0x7777_7777, 0xfedc_ba98),
        Case::preserving_flags("LEA EBP,[ECX*8-128]: no base", &[0x8d, 0x2c, 0xcd, 0x80, 0xff, 0xff, 0xff])
            .initial_register(Ecx, 0x22).register(Ebp, 0x6666_6666, 0x90),
        Case::preserving_flags("LEA EBX,[EBX]: absent index ignores scale", &[0x8d, 0x1c, 0xe3])
            .register(Ebx, 0x8000_1234, 0x8000_1234),
        Case::preserving_flags("LEA ESP,[ESP]: base through SIB", &[0x8d, 0x24, 0x24])
            .register(Esp, 0xdead_4000, 0xdead_4000),
        Case::preserving_flags("LEA EBP,[EBP+0]: displacement byte required", &[0x8d, 0x6d, 0])
            .register(Ebp, 0x8765_4321, 0x8765_4321),
        Case::preserving_flags("LEA EAX,[EBX-128]: negative displacement wraps", &[0x8d, 0x43, 0x80])
            .initial_register(Ebx, 0x10).register(Eax, 0x1111_1111, 0xffff_ff90),
        Case::preserving_flags("LEA EAX,[EBX+127]: positive displacement wraps", &[0x8d, 0x43, 0x7f])
            .initial_register(Ebx, 0xffff_fff0).register(Eax, 0x1111_1111, 0x6f),
        Case::preserving_flags("LEA EAX,[EBX+0x80000000]: high displacement bit", &[0x8d, 0x83, 0, 0, 0, 0x80])
            .initial_register(Ebx, 0x8000_4000).register(Eax, 0x1111_1111, 0x4000),
        Case::preserving_flags("LEA AX,[EBX+ECX*2+1]: word destination retains upper half", &[0x66, 0x8d, 0x44, 0x4b, 1])
            .initial_register(Ebx, 0x8000_ffff).initial_register(Ecx, 0x1234_0002).register(Eax, 0x4433_2211, 0x4433_0004),
    ]
}

test_cases!(
    base_index_and_displacement_forms_compute_addresses,
    address_cases()
);

#[rustfmt::skip]
fn scale_cases() -> Vec<Case> {
    [(0x0b, 0x4000_0011), (0x4b, 0x8000_0012), (0x8b, 0x14), (0xcb, 0x18)]
        .into_iter().map(|(sib, result)| {
            Case::preserving_flags(format!("LEA EDX,[EBX+ECX*scale+0x20]: SIB {sib:02x}"), &[0x8d, 0x54, sib, 0x20])
                .initial_register(Ebx, 0xffff_fff0)
                .initial_register(Ecx, 0x4000_0001)
                .register(Edx, 0xdead_beef, result)
        }).collect()
}
test_cases!(every_sib_scale_uses_a_wrapping_32_bit_index, scale_cases());

#[rustfmt::skip]
fn no_access_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("LEA: missing target page", &[0x8d, 0x03])
            .register(Eax, 0x1111_1111, 0x4fff).initial_register(Ebx, 0x4fff),
        Case::preserving_flags("LEA: read-only target page", &[0x8d, 0x03])
            .register(Eax, 0x1111_1111, 0x4fff).initial_register(Ebx, 0x4fff)
            .memory(0x4ffb, &[0x5a, 0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("LEA: final linear byte has no data-range check", &[0x8d, 0x03])
            .register(Eax, 0x1111_1111, 0xffff_ffff).initial_register(Ebx, 0xffff_ffff),
    ]
}
test_cases!(
    computed_addresses_do_not_access_guest_data,
    no_access_cases()
);

#[test]
fn snapshot_lea_imports_no_guest_data_or_page_table_memory() {
    for code in [&[0x8d, 0x03][..], &[0x66, 0x8d, 0x44, 0x8b, 0x80][..]] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let mut memories = Vec::new();
        for payload in Parser::new(0).parse_all(&module.bytes) {
            if let Payload::ImportSection(imports) = payload.unwrap() {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        memories.push((import.module, import.name));
                    }
                }
            }
        }
        assert_eq!(memories, [("wasm86", "cpuState")]);
    }
}

#[test]
fn register_only_modrm_forms_are_unsupported_without_extra_fetches() {
    for (code, start, exit) in [
        (&[0x8d, 0xc4][..], 0x1ffe, 0x0008_008d_0000_1ffe),
        (&[0x66, 0x8d, 0xfd][..], 0x1ffd, 0x0008_008d_0000_1ffd),
    ] {
        assert!(matches!(
            compile_block_from_bytes(start, code, 1),
            Err(BlockError::UnsupportedInstruction { address, opcode: 0x8d }) if address == start
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "LEA rejects mod=3 at the final mapped byte",
            Exit::Other(exit),
        );
    }
}

#[test]
fn snapshots_require_address_fields_but_no_immediate_or_successor() {
    for code in [
        &[0x8d, 0x03][..],
        &[0x66, 0x8d, 0x44, 0x8b, 0x80][..],
        &[0x8d, 0x04, 0x25, 0x78, 0x56, 0x34, 0x12][..],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn sequences() -> Vec<Sequence> {
    vec![Sequence::preserving_flags("mixed-width writes feed old base and index values")
        .initial_registers(&[(Eax, 0x1234_5678), (Ebx, 0x1111_4000), (Ecx, 0x8000_0108), (Esp, 0x9000_0100)])
        .step(Step::preserving_flags(&[0x66, 0xbb, 0xf0, 0xff]).register(Ebx, 0x1111_fff0))
        .step(Step::preserving_flags(&[0xb1, 4]).register(Ecx, 0x8000_0104))
        .step(Step::preserving_flags(&[0x8d, 0x44, 0x8b, 0x20]).register(Eax, 0x1112_0420))
        .step(Step::preserving_flags(&[0x66, 0x8d, 0x64, 0x40, 0x80]).register(Esp, 0x9000_0be0))
        .step(Step::preserving_flags(&[0x8d, 0x24, 0x24]).register(Esp, 0x9000_0be0))
        .step(Step::preserving_flags(&[0x8d, 0x4c, 0x8c, 0x10]).register(Ecx, 0x9000_1000))
        .step(Step::preserving_flags(&[0x66, 0x8d, 0x49, 0xff]).register(Ecx, 0x9000_0fff))
        .step(Step::preserving_flags(&[0x8d, 0x31]).register(Esi, 0x9000_0fff))]
}

test_sequences!(mixed_address_aliases, sequences());

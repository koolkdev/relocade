use wasm86_x86::{compile_block_from_bytes, Gpr32::Eax};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::support::sequences::{test_sequences, Checkpoint, SequenceCase};
#[path = "binary_operands/faults.rs"]
mod faults;
#[path = "binary_operands/memory.rs"]
mod memory;

#[test]
fn test_and_compare_only_read_guest_memory_while_updates_store() {
    for (code, writes) in [
        (&[0x38, 0x03][..], false),
        (&[0x66, 0x39, 0x03][..], false),
        (&[0x39, 0x03][..], false),
        (&[0x00, 0x03][..], true),
        (&[0x66, 0x01, 0x03][..], true),
        (&[0x01, 0x03][..], true),
        (&[0x84, 0x03][..], false),
        (&[0x66, 0x85, 0x03][..], false),
        (&[0x85, 0x03][..], false),
        (&[0xf6, 0x03, 0x80][..], false),
        (&[0x66, 0xf7, 0x03, 0x00, 0x80][..], false),
        (&[0xf7, 0x03, 0x00, 0x00, 0x00, 0x80][..], false),
        (&[0x28, 0x03][..], true),
        (&[0x66, 0x29, 0x03][..], true),
        (&[0x29, 0x03][..], true),
        (&[0x20, 0x03][..], true),
        (&[0x66, 0x21, 0x03][..], true),
        (&[0x21, 0x03][..], true),
        (&[0x08, 0x03][..], true),
        (&[0x66, 0x09, 0x03][..], true),
        (&[0x09, 0x03][..], true),
        (&[0x30, 0x03][..], true),
        (&[0x66, 0x31, 0x03][..], true),
        (&[0x31, 0x03][..], true),
    ] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut guest = None;
        let mut memory_index = 0;
        let mut loads = 0;
        let mut stores = 0;
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(section) => {
                    for import in section {
                        let import = import.unwrap();
                        if matches!(import.ty, TypeRef::Memory(_)) {
                            if import.name == "guest" {
                                guest = Some(memory_index);
                            }
                            memory_index += 1;
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::I32Load { memarg }
                            | Operator::I32Load8U { memarg }
                            | Operator::I32Load16U { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                loads += 1
                            }
                            Operator::I32Store { memarg }
                            | Operator::I32Store8 { memarg }
                            | Operator::I32Store16 { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                stores += 1
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(guest.is_some() && loads > 0, "{code:02x?}");
        assert_eq!(stores > 0, writes, "{code:02x?}");
    }
}

#[rustfmt::skip]
fn register_aliases() -> Vec<Case> {
    vec![
        Case::new("ADD AL,AH reads old AH", &[0x00, 0xe0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_7f81, 0x4433_7f00),
        Case::new("ADD AH,AL reads old AL", &[0x00, 0xc4], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x4433_8080, 0x4433_0080),
        Case::new("ADD EAX,EAX reads the old value", &[0x01, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0),
        Case::new("CMP AH,AL preserves both aliases", &[0x38, 0xc4], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7efe, 0x4433_7efe),
        Case::new("ADD AL,1 ignores the operand-size prefix", &[0x66, 0x04, 1], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_22ff, 0x4433_2200),
        Case::new("CMP AX,2211 reads two immediate bytes", &[0x66, 0x3d, 0x11, 0x22], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2211),
    ]
}

test_cases!(register_operand_aliases, register_aliases());

#[rustfmt::skip]
fn flag_preserving_alias_sequence() -> Vec<SequenceCase> {
    vec![SequenceCase::new("SETcc and MOV preserve prior ADD flags while changing aliases", Flags::all(true))
        .initial_register(Eax, 0x4433_22ff)
        .step(Checkpoint::new(&[0x04, 1],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2200))
        .step(Checkpoint::preserving_flags(&[0x66, 0x0f, 0x94, 0xfc]).register(Eax, 0x4433_0100))
        .step(Checkpoint::preserving_flags(&[0xb0, 0x7f]).register(Eax, 0x4433_017f))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc0]).register(Eax, 0x4433_0101))]
}

test_sequences!(
    setcc_and_mov_preserve_add_flags,
    flag_preserving_alias_sequence()
);

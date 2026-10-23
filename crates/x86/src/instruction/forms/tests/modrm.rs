//! Register selectors retain encoded indices and distinguish memory forms.

use super::*;
use crate::instruction::forms::declarations::{declaration_operands, operand_spec, same_layout};

fn declaration_form(operands: &[OperandSpec], modrm: Option<ModRmSelector>) -> Form {
    let handler = match operands.len() {
        1 => Handler::Unary(|_, _, _, _| Ok(())),
        2 => Handler::Binary(|_, _, _, _, _| Ok(())),
        3 => Handler::Ternary(|_, _, _, _, _, _| Ok(())),
        _ => panic!("the test declaration has one to three operands"),
    };
    Declaration {
        availability: crate::instruction::Availability::AllModes,
        features: &[],
        opcode: Opcode {
            map: OpcodeMap::Primary,
            byte: 0x00,
            prefix: PrefixRule::Modifiers(None),
            register_range: false,
            modrm,
        },
        operands,
        handlers: SizedHandlers::fixed(handler),
        effects: &[],
        lockable: false,
    }
    .form()
}

#[test]
fn rm_operands_restrict_modrm_at_every_operand_position() {
    let register = OperandSpec::ModRmRegister(RegisterClass::Gpr);
    for (rm, register_only) in [
        (operand_spec!(mem), false),
        (operand_spec!(register(rm)), true),
        (operand_spec!(register(mm_rm)), true),
        (operand_spec!(register(xmm_rm)), true),
    ] {
        for operands in [
            &[rm][..],
            &[rm, register][..],
            &[register, rm][..],
            &[rm, register, register][..],
            &[register, rm, register][..],
            &[register, register, rm][..],
        ] {
            let form = declaration_form(operands, None);
            for modrm in 0..=u8::MAX {
                assert_eq!(
                    form.matches_modrm(modrm, &PrefixState::default()),
                    (modrm >= 0xc0) == register_only
                );
            }
        }
    }
}

#[test]
fn register_rm_operands_preserve_fixed_modrm_bits() {
    for (selector, expected) in [
        (ModRmSelector::extension(5), 0xe8..=0xef),
        (ModRmSelector::register_range(0xe8), 0xe8..=0xef),
        (ModRmSelector::byte(0xeb), 0xeb..=0xeb),
    ] {
        let form = declaration_form(declaration_operands!((register(xmm_rm))), Some(selector));
        let accepted: Vec<_> = (0..=u8::MAX)
            .filter(|byte| form.matches_modrm(*byte, &PrefixState::default()))
            .collect();
        assert_eq!(accepted, expected.collect::<Vec<_>>());
    }
}

#[test]
fn declarations_reject_conflicting_modrm_constraints() {
    let register = operand_spec!(register(xmm_rm));
    for (operands, selector) in [
        (&[register, OperandSpec::Memory][..], None),
        (&[OperandSpec::Memory, register][..], None),
        (&[register, OperandSpec::Address][..], None),
        (&[OperandSpec::Address, register][..], None),
        (&[register][..], Some(ModRmSelector::byte(0x20))),
        (&[OperandSpec::Memory][..], Some(ModRmSelector::byte(0xe0))),
    ] {
        assert!(std::panic::catch_unwind(|| declaration_form(operands, selector)).is_err());
    }
}

#[test]
fn width_alternatives_must_share_rm_addressing_modes() {
    for class in [RegisterClass::Gpr, RegisterClass::Mmx, RegisterClass::Xmm] {
        let rm = OperandSpec::Rm(class);
        let register = rm.register();
        same_layout(&[register], &[register]);
        assert!(std::panic::catch_unwind(|| same_layout(&[rm], &[register])).is_err());
        assert!(std::panic::catch_unwind(|| same_layout(&[register], &[rm])).is_err());
    }
}

#[test]
fn register_only_forms_admit_both_register_fields_without_overlapping_memory() {
    let prefixes = PrefixState::default();
    for opcode in [0x12, 0x16] {
        let forms: Vec<_> = opcode_forms(OpcodeMap::Extended)
            .filter(|form| form.matches(opcode) && form.resolve(&prefixes).is_some())
            .collect();
        assert_eq!(forms.len(), 2);
        let register = forms.iter().find(|form| !form.accepts_memory_rm()).unwrap();
        let memory = forms.iter().find(|form| form.accepts_memory_rm()).unwrap();
        for modrm in 0..=u8::MAX {
            assert_eq!(register.matches_modrm(modrm, &prefixes), modrm >= 0xc0);
            assert_eq!(memory.matches_modrm(modrm, &prefixes), modrm < 0xc0);
            if modrm >= 0xc0 {
                let code = [0x0f, opcode, modrm, 0x90];
                let (decoded, remaining) =
                    crate::decode::snapshot(&code, 0x1000, crate::SegmentDefaultSize::Bits32)
                        .unwrap();
                assert_eq!(remaining, [0x90]);
                assert!(matches!(decoded.instruction.call,
                    HandlerCall::Binary {
                        left: Operand::Location(Location::Xmm(RegisterCode::Known(destination))),
                        right: Operand::Location(Location::Xmm(RegisterCode::Known(source))),
                        ..
                    } if (destination & 7) == ((modrm >> 3) & 7) && (source & 7) == (modrm & 7)));
            }
        }
    }
}

#[test]
fn modrm_register_ranges_cover_eight_stack_offsets_without_accepting_memory() {
    for (opcode, first) in [
        (0xd9, 0xc0),
        (0xd9, 0xc8),
        (0xdd, 0xc0),
        (0xdd, 0xd0),
        (0xdd, 0xd8),
    ] {
        let prefixes = PrefixState::default();
        let form = opcode_forms(OpcodeMap::Primary)
            .find(|form| form.matches(opcode) && form.matches_modrm(first, &prefixes))
            .unwrap();
        let accepted: Vec<_> = (0..=u8::MAX)
            .filter(|byte| form.matches_modrm(*byte, &prefixes))
            .collect();
        assert_eq!(accepted, (first..first + 8).collect::<Vec<_>>());
        assert!(!form.accepts_memory_rm());
        for offset in 0..8 {
            for prefix in [None, Some(0x66)] {
                let mut bytes = prefix.into_iter().collect::<Vec<_>>();
                bytes.extend_from_slice(&[opcode, first + offset, 0x62]);
                let (decoded, remaining) =
                    crate::decode::snapshot(&bytes, 0x1000, crate::SegmentDefaultSize::Bits32)
                        .unwrap();
                assert_eq!(remaining, [0x62]);
                assert_eq!(decoded.eip, 0x1000);
                assert_eq!(decoded.fallthrough_eip, 0x1000 + bytes.len() as u32 - 1);
                assert!(matches!(decoded.instruction.call,
                    HandlerCall::Unary { operand: Operand::X87StackIndex(index), .. }
                    if index == u32::from(offset)));
                let provenance = decoded.instruction.x87_opcode.unwrap();
                assert_eq!(provenance.primary, opcode);
                assert_eq!(provenance.modrm, u32::from(first + offset));
            }
        }
    }
}

#[test]
fn x87_memory_encoding_retains_modrm_before_sib_and_displacement() {
    for (modrm, expected_opcode) in [(0xac, 0x3ac), (0xbc, 0x3bc)] {
        let prefixes = PrefixState::default();
        let form = opcode_forms(OpcodeMap::Primary)
            .find(|form| form.matches(0xdb) && form.matches_modrm(modrm, &prefixes))
            .unwrap();
        assert!(!form.matches_modrm(modrm | 0xc0, &prefixes));
        let bytes = [0x66, 0x64, 0xdb, modrm, 0x93, 0x78, 0x56, 0x34, 0x12, 0x62];
        let (decoded, remaining) =
            crate::decode::snapshot(&bytes, 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
        assert_eq!(remaining, [0x62]);
        assert_eq!(decoded.fallthrough_eip, 0x1009);
        let provenance = decoded.instruction.x87_opcode.unwrap();
        assert_eq!(
            u32::from(provenance.primary & 7) << 8 | provenance.modrm,
            expected_opcode
        );
    }
    let (integer, _) =
        crate::decode::snapshot(&[0x8b, 0xc1], 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
    assert!(integer.instruction.x87_opcode.is_none());
}

#[test]
fn x87_forms_reject_lock_and_undeclared_register_encodings() {
    let mut rejected = vec![vec![0xd9, 0xd1], vec![0xdd, 0xc8], vec![0xdf, 0xc0]];
    for bytes in [
        [0xd9, 0xc0],
        [0xd9, 0xc8],
        [0xdd, 0xc0],
        [0xdd, 0xd0],
        [0xdd, 0xd8],
        [0xd9, 0xf6],
        [0xd9, 0xf7],
        [0xdb, 0xad],
        [0xdb, 0xbd],
        [0xdf, 0x05],
        [0xdb, 0x05],
        [0xdf, 0x2d],
        [0xdf, 0x15],
        [0xdb, 0x15],
        [0xdf, 0x1d],
        [0xdb, 0x1d],
        [0xdf, 0x3d],
    ] {
        let mut locked = vec![0xf0];
        locked.extend_from_slice(&bytes);
        rejected.push(locked);
    }
    for bytes in rejected {
        assert!(
            matches!(
                crate::decode::snapshot(&bytes, 0x1000, crate::SegmentDefaultSize::Bits32),
                Err(crate::BlockError::UnsupportedInstruction {
                    address: 0x1000,
                    ..
                })
            ),
            "encoding {bytes:02x?} must be rejected before any further operand bytes"
        );
    }
}

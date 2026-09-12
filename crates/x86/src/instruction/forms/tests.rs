use super::*;
use crate::{
    address::Address32,
    instruction::{
        handlers::HandlerCall, opcode_forms, Operand, EXTENDED_OPCODE_ESCAPE, OPERAND_SIZE_PREFIX,
        REPEAT_PREFIX,
    },
    register::{Gpr32, RegisterOperand},
};

fn catalog_form(map: OpcodeMap, opcode: u8, extension: Option<u8>) -> &'static Form {
    let mut candidates =
        opcode_forms(map).filter(|form| form.matches(opcode) && form.extension == extension);
    let form = candidates.next().expect("the representative form exists");
    assert!(
        candidates.next().is_none(),
        "the representative form is unique"
    );
    form
}

#[test]
fn opcode_candidates_share_a_decode_layout_and_never_overlap() {
    for map in [OpcodeMap::Primary, OpcodeMap::Extended] {
        for (opcode, candidates) in forms_by_opcode(opcode_forms(map)) {
            let has_modrm = candidates[0].encoding.has_modrm();
            let mut extensions = [false; 8];
            for form in &candidates {
                assert!(form.map == map);
                assert_eq!(form.opcode & form.mask, form.opcode);
                assert_eq!(
                    form.encoding.has_modrm(),
                    has_modrm,
                    "opcode {opcode:02x} must choose one physical decode path"
                );
                if let Some(extension) = form.extension {
                    assert!(has_modrm, "opcode extensions require ModRM");
                    assert!(extension < 8);
                    assert!(
                        !extensions[usize::from(extension)],
                        "opcode {opcode:02x} repeats extension {extension}"
                    );
                    extensions[usize::from(extension)] = true;
                } else {
                    assert_eq!(
                        candidates.len(),
                        1,
                        "opcode {opcode:02x} mixes an unrestricted form with other candidates"
                    );
                }
            }
        }
    }
}

#[test]
fn catalog_bindings_use_available_fields_and_match_both_handler_arities() {
    for map in [OpcodeMap::Primary, OpcodeMap::Extended] {
        for form in opcode_forms(map) {
            let bindings = match form.binding {
                OperandBindingShape::Nullary => vec![],
                OperandBindingShape::Unary(operand) => vec![operand],
                OperandBindingShape::Binary { left, right } => {
                    vec![OperandBinding::Location(left), right]
                }
                OperandBindingShape::Ternary {
                    destination,
                    first_source,
                    second_source,
                } => vec![
                    OperandBinding::Location(destination),
                    first_source,
                    second_source,
                ],
            };
            for size in [OperandSize::Word, OperandSize::Dword] {
                let arity = match form.with_operand_size(size).handler {
                    Handler::Nullary(_) => 0,
                    Handler::Unary(_) => 1,
                    Handler::Binary(_) => 2,
                    Handler::Ternary(_) => 3,
                };
                assert_eq!(arity, bindings.len(), "opcode {:02x}", form.opcode);
            }
            let has_immediate = matches!(
                form.encoding,
                Encoding::Immediate { .. }
                    | Encoding::OpcodeRegisterImmediate { .. }
                    | Encoding::ModRm { immediate: Some(_) }
            );
            assert_eq!(
                bindings
                    .iter()
                    .filter(|binding| matches!(binding, OperandBinding::Immediate))
                    .count(),
                usize::from(has_immediate),
                "opcode {:02x} must bind its encoded immediate exactly once",
                form.opcode
            );
            for binding in bindings {
                match binding {
                    OperandBinding::Location(LocationBinding::Register) => {
                        assert!(matches!(
                            form.encoding,
                            Encoding::OpcodeRegister
                                | Encoding::OpcodeRegisterImmediate { .. }
                                | Encoding::ModRm { .. }
                        ));
                        assert!(
                            form.extension.is_none(),
                            "ModRM.reg cannot also be an opcode extension"
                        );
                    }
                    OperandBinding::Location(LocationBinding::Rm) | OperandBinding::RmAddress => {
                        assert!(form.encoding.has_modrm());
                    }
                    OperandBinding::Location(LocationBinding::AbsoluteOffset) => {
                        assert!(matches!(form.encoding, Encoding::AccumulatorOffset));
                    }
                    OperandBinding::Immediate => assert!(has_immediate),
                    OperandBinding::Location(LocationBinding::FixedRegister(_))
                    | OperandBinding::Constant(_) => {}
                }
            }
        }
    }
}

#[test]
fn instruction_forms_cannot_shadow_decoder_prefix_and_escape_actions() {
    let primary = forms_by_opcode(opcode_forms(OpcodeMap::Primary));
    for opcode in [OPERAND_SIZE_PREFIX, REPEAT_PREFIX, EXTENDED_OPCODE_ESCAPE] {
        assert!(!primary.contains_key(&u32::from(opcode)));
    }
}

#[test]
fn immediate_encoding_keeps_fixed_widths_and_signed_bytes_distinct() {
    for (opcode, extension, word_bytes, dword_bytes, signed) in [
        (0xc2, None, 2, 2, false),
        (0xe8, None, 2, 4, false),
        (0x83, Some(0), 1, 1, true),
    ] {
        let form = catalog_form(OpcodeMap::Primary, opcode, extension);
        for (size, bytes) in [
            (OperandSize::Word, word_bytes),
            (OperandSize::Dword, dword_bytes),
        ] {
            let sized = form.with_operand_size(size);
            assert_eq!(sized.immediate_width().bytes(), bytes);
            assert_eq!(sized.sign_extends_immediate(), signed);
        }
    }

    for (bytes, fallthrough) in [
        (&[0xc2, 0xff, 0xff, 0x62][..], 0x1003),
        (&[0x66, 0xc2, 0xff, 0xff, 0x62][..], 0x1004),
    ] {
        let (decoded, remaining) = crate::decode::snapshot(bytes, 0x1000).unwrap();
        assert_eq!(remaining, [0x62]);
        assert_eq!(decoded.fallthrough_eip, fallthrough);
        assert!(decoded.instruction.ends_block());
        assert!(decoded.instruction.uses_memory());
        assert!(matches!(
            decoded.instruction.call,
            HandlerCall::Unary {
                operand: Operand::Immediate(0xffff),
                ..
            }
        ));
    }
}

#[test]
fn condition_ranges_bind_the_architectural_order_for_all_sixteen_opcodes() {
    let conditions = [
        Condition::O,
        Condition::NO,
        Condition::B,
        Condition::AE,
        Condition::E,
        Condition::NE,
        Condition::BE,
        Condition::A,
        Condition::S,
        Condition::NS,
        Condition::P,
        Condition::NP,
        Condition::L,
        Condition::GE,
        Condition::LE,
        Condition::G,
    ];
    for (map, first_opcode) in [
        (OpcodeMap::Primary, 0x70),
        (OpcodeMap::Extended, 0x40),
        (OpcodeMap::Extended, 0x80),
        (OpcodeMap::Extended, 0x90),
    ] {
        for (code, expected) in conditions.into_iter().enumerate() {
            let opcode = first_opcode + code as u8;
            let form = catalog_form(map, opcode, None);
            assert_eq!(form.opcode, opcode);
            assert_eq!(form.mask, 0xff);
            assert!(form.condition == Some(expected), "opcode {opcode:02x}");
        }
    }
}

#[test]
fn opcode_register_ranges_cover_exactly_eight_codes_and_bind_each_register() {
    for first_opcode in [0xb0, 0xb8] {
        let form = catalog_form(OpcodeMap::Primary, first_opcode, None);
        let matching: Vec<_> = (0..=u8::MAX)
            .filter(|opcode| form.matches(*opcode))
            .collect();
        assert_eq!(
            matching,
            (first_opcode..first_opcode + 8).collect::<Vec<_>>()
        );
        assert_eq!(form.mask, 0xf8);
        assert!(matches!(
            form.encoding,
            Encoding::OpcodeRegisterImmediate { .. }
        ));
        for code in 0..8 {
            for size in [OperandSize::Word, OperandSize::Dword] {
                let decoded = form.with_operand_size(size).bind(
                    DecodedFields::OpcodeRegisterImmediate {
                        register: RegisterCode::from_code(code),
                        immediate: 0x7au32,
                    },
                    0x1000,
                    0x1002,
                );
                assert!(matches!(
                    decoded.instruction.call,
                    HandlerCall::Binary {
                        left: Location::Register(RegisterOperand::Encoded(RegisterCode::Known(actual))),
                        right: Operand::Immediate(0x7a),
                        ..
                    } if actual == code
                ));
            }
        }
    }
}

#[test]
fn width_alternatives_share_one_opcode_and_preserve_implicit_register_bindings() {
    let form = catalog_form(OpcodeMap::Primary, 0x98, None);
    assert_eq!(form.mask, 0xff);
    assert!(matches!(form.encoding, Encoding::OpcodeOnly));
    for (bytes, fallthrough) in [
        (&[0x98, 0x62][..], 0x1001),
        (&[0x66, 0x98, 0x62][..], 0x1002),
    ] {
        let (decoded, remaining) = crate::decode::snapshot(bytes, 0x1000).unwrap();
        assert_eq!(remaining, [0x62]);
        assert_eq!(decoded.fallthrough_eip, fallthrough);
        assert!(!decoded.instruction.ends_block());
        assert!(!decoded.instruction.uses_memory());
        assert!(matches!(
            decoded.instruction.call,
            HandlerCall::Binary {
                left: Location::Register(RegisterOperand::Named(left)),
                right: Operand::Location(Location::Register(RegisterOperand::Named(right))),
                ..
            } if left == NamedRegister::low(Gpr32::Eax)
                && right == NamedRegister::low(Gpr32::Eax)
        ));
    }
}

#[test]
fn flag_transfer_forms_bind_ah_without_encoded_operand_fields() {
    for opcode in [0x9e, 0x9f] {
        let form = catalog_form(OpcodeMap::Primary, opcode, None);
        assert!(matches!(form.encoding, Encoding::OpcodeOnly));
        for size in [OperandSize::Word, OperandSize::Dword] {
            let decoded =
                form.with_operand_size(size)
                    .bind(DecodedFields::<u32>::OpcodeOnly, 0x1000, 0x1001);
            assert!(!decoded.instruction.ends_block());
            assert!(!decoded.instruction.uses_memory());
            assert!(matches!(
                decoded.instruction.call,
                HandlerCall::Unary {
                    operand: Operand::Location(Location::Register(RegisterOperand::Named(register))),
                    ..
                } if register == NamedRegister::AH
            ));
        }
    }
}

#[test]
fn effective_address_binding_rejects_register_modes_without_claiming_a_memory_read() {
    let lea = catalog_form(OpcodeMap::Primary, 0x8d, None);
    let mov = catalog_form(OpcodeMap::Primary, 0x8b, None);
    for modrm in 0..=u8::MAX {
        assert_eq!(lea.matches_modrm(modrm), modrm >> 6 != 3);
        assert!(mov.matches_modrm(modrm));
    }
    for size in [OperandSize::Word, OperandSize::Dword] {
        let fields = || DecodedFields::ModRm {
            register: RegisterCode::from_code(2),
            rm: Location::Memory(Address32 {
                base: None,
                index: None,
                displacement: 0x12345678u32,
            }),
            immediate: None,
        };
        let address = lea.with_operand_size(size).bind(fields(), 0x1000, 0x1006);
        assert!(!address.instruction.uses_memory());
        assert!(matches!(
            address.instruction.call,
            HandlerCall::Binary {
                right: Operand::Address(Address32 {
                    displacement: 0x12345678,
                    ..
                }),
                ..
            }
        ));
        let load = mov.with_operand_size(size).bind(fields(), 0x1000, 0x1006);
        assert!(load.instruction.uses_memory());
        assert!(matches!(
            load.instruction.call,
            HandlerCall::Binary {
                right: Operand::Location(Location::Memory(Address32 {
                    displacement: 0x12345678,
                    ..
                })),
                ..
            }
        ));
    }
}

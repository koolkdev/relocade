mod immediates;

use super::declarations::{Declaration, Opcode, OperandSpec};
use super::*;
use crate::{
    address::EffectiveAddress,
    instruction::{
        handlers::{Handler, HandlerCall},
        opcode_forms, Location, Operand, Prefix, PrefixState, EXTENDED_OPCODE_ESCAPE,
    },
    register::{Gpr32, RegisterCode, RegisterOperand, RegisterSelection},
};

fn word_prefixes() -> PrefixState {
    PrefixState::default().with_prefix(Prefix::OperandSize)
}

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
fn catalog_bindings_select_declared_fields() {
    for map in [OpcodeMap::Primary, OpcodeMap::Extended] {
        for form in opcode_forms(map) {
            for resolved in PrefixState::combinations(crate::SegmentDefaultSize::Bits32)
                .filter_map(|prefixes| form.resolve(&prefixes))
            {
                let bindings = match resolved.call {
                    HandlerCall::Nullary { .. } => vec![],
                    HandlerCall::Unary { operand, .. } => vec![operand],
                    HandlerCall::Binary { left, right, .. } => vec![left, right],
                    HandlerCall::Ternary {
                        destination,
                        first_source,
                        second_source,
                        ..
                    } => vec![
                        OperandBinding::Location(destination),
                        first_source,
                        second_source,
                    ],
                };
                assert_eq!(
                    bindings
                        .iter()
                        .filter_map(|binding| match binding {
                            OperandBinding::Immediate(index) => Some(*index),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                    form.encoding
                        .immediates
                        .iter()
                        .enumerate()
                        .filter_map(|(index, width)| width.map(|_| index))
                        .collect::<Vec<_>>(),
                    "opcode {:02x} must bind each encoded immediate once in order",
                    form.opcode
                );
                for binding in bindings {
                    match binding {
                        OperandBinding::Location(LocationBinding::Register) => {
                            assert!(matches!(
                                form.encoding.operands,
                                OperandEncoding::OpcodeRegister | OperandEncoding::ModRm
                            ));
                            assert!(
                                form.extension.is_none(),
                                "ModRM.reg cannot also be an opcode extension"
                            );
                        }
                        OperandBinding::Location(LocationBinding::Rm)
                        | OperandBinding::RmAddress => {
                            assert!(form.encoding.has_modrm());
                        }
                        OperandBinding::Location(LocationBinding::AbsoluteOffset) => {
                            assert!(matches!(
                                form.encoding.operands,
                                OperandEncoding::AbsoluteOffset
                            ));
                        }
                        OperandBinding::Immediate(index) => {
                            assert!(form.encoding.immediates[index].is_some())
                        }
                        OperandBinding::Location(LocationBinding::FixedRegister(_))
                        | OperandBinding::Segment(_)
                        | OperandBinding::Constant(_) => {}
                    }
                }
            }
        }
    }
}

#[test]
fn instruction_forms_cannot_shadow_decoder_prefix_and_escape_actions() {
    let primary = forms_by_opcode(opcode_forms(OpcodeMap::Primary));
    for opcode in Prefix::ALL
        .into_iter()
        .map(Prefix::byte)
        .chain([EXTENDED_OPCODE_ESCAPE])
    {
        assert!(!primary.contains_key(&u32::from(opcode)));
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
            form.encoding.operands,
            OperandEncoding::OpcodeRegister
        ));
        for code in 0..8 {
            for prefixes in [word_prefixes(), PrefixState::default()] {
                let decoded = form.resolve(&prefixes).unwrap().bind(
                    DecodedFields {
                        register: Some(RegisterCode::from_code(code)),
                        immediates: [Some(0x7au32), None],
                        ..DecodedFields::default()
                    },
                    0x1000,
                    0x1002,
                );
                assert!(matches!(
                    decoded.instruction.call,
                    HandlerCall::Binary {
                        left: Operand::Location(Location::Register(RegisterOperand::Encoded(RegisterCode::Known(actual)))),
                        right: Operand::Immediate(0x7a),
                        ..
                    } if actual == code
                ));
            }
        }
    }
}

#[test]
fn modrm_direction_selects_the_handler_destination_and_source() {
    for (opcode, destination, source) in [
        (0x89, Gpr32::Ecx, Gpr32::Ebx),
        (0x8b, Gpr32::Ebx, Gpr32::Ecx),
    ] {
        // ModRM encodes EBX in reg and ECX in r/m; the opcode determines direction.
        let bytes = [opcode, 0xd9];
        let (decoded, remaining) =
            crate::decode::snapshot(&bytes, 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
        assert!(remaining.is_empty());
        let HandlerCall::Binary {
            left: Operand::Location(Location::Register(left)),
            right: Operand::Location(Location::Register(right)),
            ..
        } = decoded.instruction.call
        else {
            panic!("register MOV binds two register operands");
        };
        assert!(matches!(left.view::<wasm86_compiler::I32>().selection,
            RegisterSelection::Named { parent, byte: 0 } if parent == destination));
        assert!(matches!(right.view::<wasm86_compiler::I32>().selection,
            RegisterSelection::Named { parent, byte: 0 } if parent == source));
    }
}

#[test]
fn width_alternatives_share_one_opcode_and_preserve_implicit_register_bindings() {
    let form = catalog_form(OpcodeMap::Primary, 0x98, None);
    assert_eq!(form.mask, 0xff);
    assert!(matches!(form.encoding.operands, OperandEncoding::None));
    for (bytes, fallthrough) in [
        (&[0x98, 0x62][..], 0x1001),
        (&[0x66, 0x98, 0x62][..], 0x1002),
    ] {
        let (decoded, remaining) =
            crate::decode::snapshot(bytes, 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
        assert_eq!(remaining, [0x62]);
        assert_eq!(decoded.fallthrough_eip, fallthrough);
        assert!(!decoded.instruction.ends_block());
        assert!(!decoded.instruction.uses_memory());
        assert!(matches!(
            decoded.instruction.call,
            HandlerCall::Binary {
                left: Operand::Location(Location::Register(RegisterOperand::Named(left))),
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
        assert!(matches!(form.encoding.operands, OperandEncoding::None));
        for prefixes in [word_prefixes(), PrefixState::default()] {
            let decoded = form.resolve(&prefixes).unwrap().bind(
                DecodedFields::<u32>::default(),
                0x1000,
                0x1001,
            );
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
    for prefixes in [word_prefixes(), PrefixState::default()] {
        let fields = || DecodedFields {
            register: Some(RegisterCode::from_code(2)),
            rm: Some(Location::Memory(
                EffectiveAddress {
                    size: crate::address::AddressSize::Bits32,
                    base: None,
                    index: None,
                    displacement: 0x12345678u32,
                }
                .memory()
                .into(),
            )),
            ..DecodedFields::default()
        };
        let address = lea
            .resolve(&prefixes)
            .unwrap()
            .bind(fields(), 0x1000, 0x1006);
        assert!(!address.instruction.uses_memory());
        assert!(matches!(
            address.instruction.call,
            HandlerCall::Binary {
                right: Operand::Address(EffectiveAddress {
                    displacement: 0x12345678,
                    ..
                }),
                ..
            }
        ));
        let load = mov
            .resolve(&prefixes)
            .unwrap()
            .bind(fields(), 0x1000, 0x1006);
        assert!(load.instruction.uses_memory());
        assert!(matches!(
            load.instruction.call,
            HandlerCall::Binary {
                right: Operand::Location(Location::Memory(address)),
                ..
            } if address.offset.displacement == 0x12345678
        ));
    }
}

#[test]
fn memory_only_bindings_restrict_modrm_at_every_operand_position() {
    let memory = OperandSpec::Memory;
    let register = OperandSpec::ModRmRegister;
    let unary = Handler::Unary(|_, _, _, fallthrough| Ok(fallthrough));
    let binary = Handler::Binary(|_, _, _, _, fallthrough| Ok(fallthrough));
    let ternary = Handler::Ternary(|_, _, _, _, _, fallthrough| Ok(fallthrough));
    for (operands, handler) in [
        (&[memory][..], unary),
        (&[memory, register][..], binary),
        (&[register, memory][..], binary),
        (&[memory, register, register][..], ternary),
        (&[register, memory, register][..], ternary),
        (&[register, register, memory][..], ternary),
    ] {
        let form = Declaration {
            opcode: Opcode {
                map: OpcodeMap::Primary,
                byte: 0x00,
                register_range: false,
                extension: None,
            },
            operands,
            handlers: SizedHandlers::fixed(handler),
            effects: &[],
            repeat_handlers: None,
        }
        .form();
        for modrm in 0..=u8::MAX {
            assert_eq!(form.matches_modrm(modrm), modrm >> 6 != 3);
        }
    }
}

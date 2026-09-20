//! Ordered physical fields retain their widths and logical operand bindings.

use super::declarations::{Declaration, Opcode, OperandSpec};
use super::*;

#[test]
fn two_immediates_bind_in_encoded_order_with_independent_physical_widths() {
    let form = catalog_form(OpcodeMap::Primary, 0xea, None);
    for (prefixes, offset_bytes) in [(word_prefixes(), 2), (PrefixState::default(), 4)] {
        let resolved = form.resolve(&prefixes).unwrap();
        let fields = resolved.immediates();
        assert_eq!(fields.len(), 2);
        assert_eq!(
            resolved.immediate_width(*fields.get(0)).bytes(),
            offset_bytes
        );
        assert_eq!(resolved.immediate_width(*fields.get(1)).bytes(), 2);
    }
    for (bytes, offset, fallthrough) in [
        (
            &[0xea, 0x78, 0x56, 0x34, 0x92, 0x27, 0xf3, 0x62][..],
            0x9234_5678,
            0x1007,
        ),
        (
            &[0x66, 0xea, 0x78, 0x56, 0x27, 0xf3, 0x62][..],
            0x5678,
            0x1006,
        ),
    ] {
        let (decoded, remaining) =
            crate::decode::snapshot(bytes, 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
        assert_eq!(remaining, [0x62]);
        assert_eq!(decoded.fallthrough_eip, fallthrough);
        assert!(decoded.instruction.ends_block());
        assert!(!decoded.instruction.uses_memory());
        assert!(matches!(decoded.instruction.call, HandlerCall::Binary {
            left: Operand::Immediate(actual), right: Operand::Immediate(0xf327), ..
        } if actual == offset));
    }
}

#[test]
fn immediate_indices_skip_locations_in_every_layout_that_accepts_immediates() {
    for location in [
        OperandSpec::FixedRegister(NamedRegister::low(Gpr32::Eax)),
        OperandSpec::OpcodeRegister,
        OperandSpec::Rm,
    ] {
        for two in [false, true] {
            let operands = [
                location,
                OperandSpec::Immediate(ImmediateWidth::OperandSize),
                OperandSpec::Immediate(ImmediateWidth::Word),
            ];
            let form = Declaration {
                opcode: Opcode {
                    map: OpcodeMap::Primary,
                    byte: 0x00,
                    register_range: matches!(location, OperandSpec::OpcodeRegister),
                    extension: None,
                },
                operands: &operands[..if two { 3 } else { 2 }],
                handlers: SizedHandlers::fixed(if two {
                    Handler::Ternary(|_, _, _, _, _, fallthrough| Ok(fallthrough))
                } else {
                    Handler::Binary(|_, _, _, _, fallthrough| Ok(fallthrough))
                }),
                effects: &[],
                repeat_handlers: None,
            }
            .form();
            let immediates: ImmediateFields<u32> = if two {
                ImmediateFields::Two(0x9234_5678, 0xf327)
            } else {
                ImmediateFields::One(0x9234_5678)
            };
            let fields = match location {
                OperandSpec::OpcodeRegister => DecodedFields::OpcodeRegisterImmediate {
                    register: RegisterCode::from_code(0),
                    immediates,
                },
                OperandSpec::Rm => DecodedFields::ModRm {
                    register: RegisterCode::from_code(0),
                    rm: Location::Register(RegisterCode::from_code(0).into()),
                    immediates: Some(immediates),
                },
                _ => DecodedFields::Immediate { immediates },
            };
            let decoded = form
                .resolve(&PrefixState::default())
                .unwrap()
                .bind(fields, 0x1000, 0x1007);
            if two {
                assert!(matches!(
                    decoded.instruction.call,
                    HandlerCall::Ternary {
                        destination: Location::Register(_),
                        first_source: Operand::Immediate(0x9234_5678),
                        second_source: Operand::Immediate(0xf327),
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    decoded.instruction.call,
                    HandlerCall::Binary {
                        left: Operand::Location(Location::Register(_)),
                        right: Operand::Immediate(0x9234_5678),
                        ..
                    }
                ));
            }
        }
    }
}

#[test]
fn a_failed_field_read_prevents_reading_later_fields() {
    let mut reads = vec![];
    let result = ImmediateFields::Two(0, 1).try_map(|field| {
        reads.push(field);
        Err::<(), _>("missing first field")
    });
    assert!(matches!(result, Err("missing first field")));
    assert_eq!(reads, [0]);
}

#[test]
fn immediate_encoding_keeps_fixed_widths_and_signed_bytes_distinct() {
    for (opcode, extension, word_bytes, dword_bytes, signed) in [
        (0xc2, None, 2, 2, false),
        (0xe8, None, 2, 4, false),
        (0x83, Some(0), 1, 1, true),
    ] {
        let form = catalog_form(OpcodeMap::Primary, opcode, extension);
        for (prefixes, bytes) in [
            (word_prefixes(), word_bytes),
            (PrefixState::default(), dword_bytes),
        ] {
            let resolved = form.resolve(&prefixes).unwrap();
            let fields = resolved.immediates();
            assert_eq!(fields.len(), 1);
            assert_eq!(resolved.immediate_width(*fields.get(0)).bytes(), bytes);
            assert_eq!(fields.get(0).is_signed(), signed);
        }
    }

    for (bytes, fallthrough) in [
        (&[0xc2, 0xff, 0xff, 0x62][..], 0x1003),
        (&[0x66, 0xc2, 0xff, 0xff, 0x62][..], 0x1004),
    ] {
        let (decoded, remaining) =
            crate::decode::snapshot(bytes, 0x1000, crate::SegmentDefaultSize::Bits32).unwrap();
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

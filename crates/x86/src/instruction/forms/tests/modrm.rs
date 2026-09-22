//! Register ranges retain their encoded indices and original opcode bytes.

use super::*;

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
                assert!(!decoded.instruction.uses_memory());
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
        assert!(decoded.instruction.uses_memory());
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

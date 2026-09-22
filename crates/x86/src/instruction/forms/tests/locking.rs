//! The P4 LOCK encoding set is independent of the semantic handlers it selects.

use crate::{decode, BlockError, SegmentDefaultSize};

#[test]
fn lock_admits_each_memory_destination_encoding_and_rejects_register_modes() {
    let mut encodings = Vec::new();
    for opcode in [
        0x00, 0x01, 0x08, 0x09, 0x10, 0x11, 0x18, 0x19, 0x20, 0x21, 0x28, 0x29, 0x30, 0x31, 0x86,
        0x87,
    ] {
        encodings.push((vec![opcode], 0x03));
    }
    for opcode in [0x80, 0x81, 0x83] {
        for extension in 0..=6 {
            encodings.push((vec![opcode], extension << 3 | 3));
        }
    }
    for opcode in [0xfe, 0xff] {
        for extension in [0, 1] {
            encodings.push((vec![opcode], extension << 3 | 3));
        }
    }
    for opcode in [0xf6, 0xf7] {
        for extension in [2, 3] {
            encodings.push((vec![opcode], extension << 3 | 3));
        }
    }
    for opcode in [0xc0, 0xc1, 0xb0, 0xb1, 0xab, 0xb3, 0xbb] {
        encodings.push((vec![0x0f, opcode], 0x03));
    }
    for extension in [5, 6, 7] {
        encodings.push((vec![0x0f, 0xba], extension << 3 | 3));
    }
    encodings.push((vec![0x0f, 0xc7], 0x0b));
    for (opcode, modrm) in encodings {
        for operand_override in [false, true] {
            let mut code = vec![0xf0];
            if operand_override {
                code.push(0x66);
            }
            code.extend_from_slice(&opcode);
            let modrm_position = code.len();
            code.push(modrm);
            code.extend_from_slice(&[0; 4]);
            let (decoded, _) = decode::snapshot(&code, 0x1000, SegmentDefaultSize::Bits32)
                .unwrap_or_else(|error| panic!("LOCK memory form {code:02x?}: {error}"));
            assert!(decoded.instruction.uses_memory());
            code[modrm_position] |= 0xc0;
            assert!(
                matches!(
                    decode::snapshot(&code, 0x1000, SegmentDefaultSize::Bits32),
                    Err(BlockError::UnsupportedInstruction {
                        address: 0x1000,
                        opcode: 0xf0
                    })
                ),
                "LOCK register form {code:02x?}"
            );
        }
    }
}

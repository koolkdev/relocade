use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, BlockError, CompiledModule};
use wasmparser::Validator;

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[allow(dead_code)]
#[path = "support/machine.rs"]
mod machine;
use machine::{both, check, Exit, Image, Step};

#[test]
fn arithmetic_lengths_follow_the_selected_operand_and_immediate_widths() {
    for code in [
        &[0x00, 0xd8][..],
        &[0x01, 0xd8],
        &[0x02, 0xc3],
        &[0x03, 0xc3],
        &[0x04, 0x80],
        &[0x05, 0x80, 0x81, 0x83, 0x66],
        &[0x38, 0xd8],
        &[0x39, 0xd8],
        &[0x3a, 0xc3],
        &[0x3b, 0xc3],
        &[0x3c, 0x80],
        &[0x3d, 0x80, 0x81, 0x83, 0x66],
        &[0x80, 0xc0, 0x80],
        &[0x80, 0xf8, 0xff],
        &[0x81, 0xc0, 0x80, 0x81, 0x83, 0x66],
        &[0x81, 0xf8, 0x80, 0x81, 0x83, 0x66],
        &[0x83, 0xc0, 0xff],
        &[0x83, 0xf8, 0x80],
        &[0x66, 0x05, 0x80, 0x81],
        &[0x66, 0x3d, 0x80, 0x81],
        &[0x66, 0x81, 0xc0, 0x80, 0x81],
        &[0x66, 0x81, 0xf8, 0x80, 0x81],
        &[0x66, 0x83, 0xc0, 0xff],
        &[0x66, 0x83, 0xf8, 0x80],
        &[0x66, 0x80, 0xc0, 0x80],
        &[0x66, 0x0f, 0x9f, 0xc4],
        &[0x81, 0x84, 0x8b, 0, 0x40, 0, 0, 0x80, 0x81, 0x83, 0x66],
        &[0x0f, 0x94, 0x84, 0x8b, 0, 0x40, 0, 0],
    ] {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual }) if actual == available
                ),
                "{code:02x?}, available {available}"
            );
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut with_suffix = code.to_vec();
        with_suffix.push(0x0f);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes
        );
    }
}

#[test]
fn setcc_ignores_modrm_reg_without_changing_its_destination() {
    for condition in 0..16 {
        let base = compile_block_from_bytes(0x1000, &[0x0f, 0x90 + condition, 0xc4], 1).unwrap();
        Validator::new().validate_all(&base.bytes).unwrap();
        let ignored = (condition & 7) << 3;
        let equivalent =
            compile_block_from_bytes(0x1000, &[0x0f, 0x90 + condition, 0xc4 | ignored], 1).unwrap();
        assert_eq!(base.bytes, equivalent.bytes, "condition {condition}");
    }
}

#[test]
fn unsupported_extensions_stop_before_address_and_immediate_fields() {
    for (code, opcode) in [
        (&[0x80, 0x0c][..], 0x80),
        (&[0x81, 0x34][..], 0x81),
        (&[0x66, 0x83, 0x2d][..], 0x83),
        (&[0x0f, 0x0b][..], 0x0f),
        (&[0x66, 0x0f, 0xff][..], 0x0f),
    ] {
        assert_eq!(
            compile_block_from_bytes(0x1000, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode
            })
        );
    }
    for (prefixes, suffix) in [
        (14, &[0x0f][..]),
        (13, &[0x0f, 0x94][..]),
        (12, &[0x0f, 0x94, 0x04][..]),
        (12, &[0x81, 0xc0, 1][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert_eq!(code.len(), 15);
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 })
        );
    }
}

fn execute(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    for (name, start, code, fault) in [
        (
            "missing second opcode",
            0x1fff,
            vec![0x0f],
            0x0004_0010_0000_2000,
        ),
        (
            "missing SETcc ModRM",
            0x1ffe,
            vec![0x0f, 0x94],
            0x0004_0010_0000_2000,
        ),
        (
            "missing SETcc SIB",
            0x1ffd,
            vec![0x0f, 0x94, 0x04],
            0x0004_0010_0000_2000,
        ),
        (
            "missing SETcc displacement before data denial",
            0x1ffa,
            vec![0x0f, 0x94, 0x05, 0, 0x40, 0],
            0x0004_0010_0000_2000,
        ),
        (
            "missing ADD immediate before data denial",
            0x1ff7,
            vec![0x81, 0x04, 0x25, 0, 0x40, 0, 0, 1, 0],
            0x0004_0010_0000_2000,
        ),
        (
            "unsupported ADD group extension before SIB",
            0x1ffe,
            vec![0x81, 0x0c],
            0x0008_0081_0000_1ffe,
        ),
        (
            "unsupported second opcode before ModRM",
            0x1ffe,
            vec![0x0f, 0x0b],
            0x0008_000f_0000_1ffe,
        ),
        (
            "second opcode beyond length limit",
            0x1ff1,
            [vec![0x66; 14], vec![0x0f]].concat(),
            0x0002_0000_0000_0000,
        ),
        (
            "SETcc ModRM beyond length limit",
            0x1ff1,
            [vec![0x66; 13], vec![0x0f, 0x94]].concat(),
            0x0002_0000_0000_0000,
        ),
        (
            "SETcc SIB beyond length limit",
            0x1ff1,
            [vec![0x66; 12], vec![0x0f, 0x94, 0x04]].concat(),
            0x0002_0000_0000_0000,
        ),
        (
            "ADD immediate beyond length limit",
            0x1ff1,
            [vec![0x66; 12], vec![0x81, 0xc0, 1]].concat(),
            0x0002_0000_0000_0000,
        ),
        (
            "last-byte unsupported group avoids a length fault",
            0x1ff1,
            [vec![0x66; 13], vec![0x81, 0x0c]].concat(),
            0x0008_0081_0000_1ff1,
        ),
        (
            "last-byte second opcode rejection avoids a length fault",
            0x1ff1,
            [vec![0x66; 13], vec![0x0f, 0x0b]].concat(),
            0x0008_000f_0000_1ff1,
        ),
    ] {
        let mut image = Image::new(&[]);
        image.cpu[0] = 0;
        image.register(56, start);
        image.register(36, 0x4000);
        image.data(0x3000 + (start & 0xfff), &code);
        check(
            &step,
            flags,
            name,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(fault),
            }],
        );
    }
    let code = [0x0f, 0x94, 0xc4];
    let mut image = Image::new(&[]);
    image.cpu[0] = 0;
    image.cpu[15] = 1;
    image.register(24, 0x4433_2211);
    image.register(56, 0x1fff);
    image.map(2, 0xa000, false);
    image.data(0x3fff, &code[..1]);
    image.data(0xa000, &code[1..]);
    both(
        &step,
        flags,
        "extended opcode spans scattered code pages",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(24, 0x4433_0111), (56, 0x2002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2002),
        }],
    );

    let code = [0x05, 1, 0, 0, 0];
    let mut image = Image::new(&[]);
    image.cpu[0] = 0;
    image.register(24, 0xffff_ffff);
    image.register(56, 0xffff_fffd);
    image.map(0xfffff, 0x8000, false);
    image.map(0, 0xa000, false);
    image.data(0x8ffd, &code[..3]);
    image.data(0xa000, &code[3..]);
    both(
        &step,
        flags,
        "ADD immediate wraps instruction addresses",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[
                (0, 0xa5a5_a50a),
                (4, 0xffff_ffff),
                (8, 1),
                (24, 0),
                (56, 2),
                (144, 0),
            ],
            ram: &[],
            exit: Exit::Dispatch(2),
        }],
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_fetch_precedence_executes_in_v8() {
    execute(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_fetch_precedence_executes_in_optimizing_v8() {
    execute(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}

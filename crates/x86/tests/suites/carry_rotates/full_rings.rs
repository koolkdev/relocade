use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{both, check, expected as observe_expected, Image, Step},
    step::TestModule,
};

use super::{image, retire, OPERATIONS};

#[test]
fn full_carry_rings_preserve_every_byte_of_concrete_and_lazy_flag_records() {
    for (bits, counts) in [(8, &[9, 18, 27, 41, 50, 59][..]), (16, &[17, 49][..])] {
        for operation in OPERATIONS {
            for carry in [0, 1] {
                for lazy in [false, true] {
                    for &count in counts {
                        for from_cl in [false, true] {
                            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                            code.extend_from_slice(&[
                                if from_cl { 0xd2 } else { 0xc0 } + u8::from(bits != 8),
                                0xc0 | (operation.extension() << 3),
                            ]);
                            if !from_cl {
                                code.push(count);
                            }
                            let mut image = image(&code, carry);
                            image.cpu.registers.eax = 0x4433_8081;
                            image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                            image.cpu.flags.reserved = [0x12, 0x34, 0x56];
                            if lazy {
                                // Both byte additions have OF=1: 127+1 has CF=0,
                                // and 128+128 has CF=1. The stale bytes disagree.
                                image.cpu.flags.kind = 2;
                                image.cpu.flags.left = if carry == 0 { 0x7f } else { 0x80 };
                                image.cpu.flags.right = if carry == 0 { 1 } else { 0x80 };
                                image.cpu.flags.status.cf = 0xfe | (1 - carry);
                                image.cpu.flags.status.of = 0x5a;
                            }
                            // The complete raw record and destination are literal
                            // no-ops; this expectation does not use the rotate oracle.
                            let mut cpu = image.cpu;
                            let step = retire(&mut cpu, code.len() as u32, &[]);
                            both(
                                TestModule::interpreter(),
                                &format!(
                                    "{operation:?} {bits}-bit by {count}, CF {carry}, \
                                     OF 1, lazy {lazy}, CL {from_cl}"
                                ),
                                &code,
                                1,
                                &image,
                                &[step],
                            );
                        }
                    }
                }
            }
        }
    }
}

struct Sequence {
    code: Vec<u8>,
    image: Image,
    steps: Vec<Step<'static>>,
    block_step: Step<'static>,
}

// ADD supplies OF=1 with either carry. Optional INC replaces five flags while
// retaining that carry, so the full ring must also preserve a pending partial change.
fn pending_sequence(rotate: &[u8], count: u8, carry: u8, increment: bool) -> Sequence {
    let mut code = vec![0x00, 0xf2]; // ADD DL,DH
    if increment {
        code.push(0x46); // INC ESI
    }
    code.extend_from_slice(rotate);
    code.extend_from_slice(&[
        0x0f, 0x90, 0xc3, // SETO BL
        0x0f, 0x92, 0xc7, // SETC BH
    ]);
    let mut image = image(&code, 1 - carry);
    image.cpu.flags.status.of = 0x5a;
    image.cpu.flags.reserved = [0x12, 0x34, 0x56];
    image.cpu.registers.eax = 0x4433_8081;
    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
    image.cpu.registers.edx = if carry == 0 { 0xccbb_017f } else { 0xccbb_8080 };
    image.cpu.registers.esi = 0x7fff_ffff;
    let mut cpu = image.cpu;
    cpu.registers.edx = if carry == 0 { 0xccbb_0180 } else { 0xccbb_8000 };
    cpu.flags.kind = 2;
    cpu.flags.left = if carry == 0 { 0x7f } else { 0x80 };
    cpu.flags.right = if carry == 0 { 1 } else { 0x80 };
    let mut steps = vec![retire(&mut cpu, 2, &[])];
    if increment {
        cpu.registers.esi = 0x8000_0000;
        cpu.flags.kind = 0;
        cpu.flags.status = StatusFlags {
            cf: carry,
            pf: 1,
            af: 1,
            zf: 0,
            sf: 1,
            of: 1,
        };
        steps.push(retire(&mut cpu, 1, &[]));
    }
    steps.push(retire(&mut cpu, rotate.len() as u32, &[]));
    cpu.registers.ebx = 0x10ff_ee01;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.ebx = 0x10ff_0001 | (u32::from(carry) << 8);
    steps.push(retire(&mut cpu, 3, &[]));
    if increment {
        // Only interpreter boundaries published ADD's lazy operands. The
        // block's concrete partial result retains the old unused payload.
        cpu.flags.left = image.cpu.flags.left;
        cpu.flags.right = image.cpu.flags.right;
    }
    let block_step = Step {
        cpu,
        ram: &[],
        exit: steps.last().unwrap().exit,
    };
    Sequence {
        code,
        image,
        steps,
        block_step,
    }
}

#[test]
fn full_carry_rings_preserve_pending_overflow_and_carry_for_conditions() {
    for (name, rotate, count) in [
        ("RCL AL,CL by 9", &[0xd2, 0xd0][..], 9),
        ("RCR AL,CL by 9", &[0xd2, 0xd8][..], 9),
        ("RCL AL,18", &[0xc0, 0xd0, 18][..], 18),
        ("RCR AL,27", &[0xc0, 0xd8, 27][..], 27),
        ("RCL AX,CL by 17", &[0x66, 0xd3, 0xd0][..], 17),
        ("RCR AX,CL by 17", &[0x66, 0xd3, 0xd8][..], 17),
        ("RCL AX,17", &[0x66, 0xc1, 0xd0, 17][..], 17),
        ("RCR AX,17", &[0x66, 0xc1, 0xd8, 17][..], 17),
    ] {
        for carry in [0, 1] {
            for increment in [false, true] {
                let sequence = pending_sequence(rotate, count, carry, increment);
                let name = format!("{name}, CF {carry}, pending INC {increment}");
                check(
                    TestModule::interpreter(),
                    &name,
                    &sequence.image,
                    &sequence.steps,
                );
                let block = TestModule::new(
                    &compile_block_from_bytes(
                        sequence.image.cpu.eip,
                        &sequence.code,
                        sequence.steps.len() as u32,
                    )
                    .unwrap(),
                );
                check(&block, &name, &sequence.image, &[sequence.block_step]);
            }
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn full_carry_rings_preserve_pending_overflow_in_optimizing_v8() {
    for (name, rotate, count, carry, increment) in [
        ("RCL AL,CL", &[0xd2, 0xd0][..], 9, 0, false),
        ("RCR AL,27", &[0xc0, 0xd8, 27][..], 27, 1, false),
        ("RCL AX,17", &[0x66, 0xc1, 0xd0, 17][..], 17, 1, true),
        ("RCR AX,CL", &[0x66, 0xd3, 0xd8][..], 17, 0, true),
    ] {
        let sequence = pending_sequence(rotate, count, carry, increment);
        assert_eq!(
            TestModule::interpreter().observe_v8(&sequence.image.input(), sequence.steps.len()),
            observe_expected(&sequence.image, &sequence.steps),
            "interpreter {name}, CF {carry}, pending INC {increment}"
        );
        let block = TestModule::new(
            &compile_block_from_bytes(
                sequence.image.cpu.eip,
                &sequence.code,
                sequence.steps.len() as u32,
            )
            .unwrap(),
        );
        assert_eq!(
            block.observe_v8(&sequence.image.input(), 1),
            observe_expected(&sequence.image, &[sequence.block_step]),
            "snapshot {name}, CF {carry}, pending INC {increment}"
        );
    }
}

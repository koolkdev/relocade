use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected as rotate_expected, image, retire, Operation, PRIOR_FLAGS};

#[test]
fn pending_add_flags_survive_zero_rotate_and_feed_a_condition() {
    for rotate in [0xc4, 0xcc] {
        let code = [0x00, 0xd0, 0xd2, rotate, 0x0f, 0x90, 0xc3];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4433_817f;
        image.cpu.registers.ecx = 0x8877_6620;
        image.cpu.registers.edx = 0xccbb_aa01;
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x4433_8180;
        cpu.flags.kind = 2;
        cpu.flags.left = 0x7f;
        cpu.flags.right = 1;
        let mut steps = vec![retire(&mut cpu, 2, &[])];
        steps.push(retire(&mut cpu, 2, &[]));
        cpu.registers.ebx = 0x10ff_ee01;
        steps.push(retire(&mut cpu, 3, &[]));
        both(
            TestModule::interpreter(),
            "zero rotate keeps pending ADD overflow",
            &code,
            3,
            &image,
            &steps,
        );
    }
}

#[test]
fn rotate_carry_feeds_subtract_with_borrow_and_a_condition() {
    for (operation, input) in [(Operation::Rol, 0x80), (Operation::Ror, 1)] {
        for count in [0, 1, 8] {
            let code = [
                0xc0,
                0xc0 | (operation.extension() << 3),
                count,
                0x83,
                0xda,
                0, // SBB EDX,0
                0x0f,
                0x92,
                0xc3, // SETC BL
            ];
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4433_2200 | input;
            image.cpu.registers.edx = 0;
            let mut cpu = image.cpu;
            let rotated = rotate_expected(operation, 8, input, count, PRIOR_FLAGS);
            cpu.registers.eax = 0x4433_2200 | rotated.value;
            rotated.apply_flags(&mut cpu);
            let carry = rotated.status.unwrap_or(PRIOR_FLAGS).cf;
            let mut steps = vec![retire(&mut cpu, 3, &[])];
            cpu.registers.edx = if carry == 1 { u32::MAX } else { 0 };
            cpu.flags.kind = 0;
            cpu.flags.status = StatusFlags {
                cf: carry,
                pf: 1,
                af: carry,
                zf: 1 - carry,
                sf: carry,
                of: 0,
            };
            steps.push(retire(&mut cpu, 3, &[]));
            cpu.registers.ebx = 0x10ff_ee00 | u32::from(carry);
            steps.push(retire(&mut cpu, 3, &[]));
            both(
                TestModule::interpreter(),
                "rotate carry reaches SBB and SETC",
                &code,
                3,
                &image,
                &steps,
            );
        }
    }
}

#[test]
fn add_rotate_and_increment_feed_conditions_and_adc() {
    let code = [
        0x00, 0xd0, // ADD AL,DL: zero with carry and auxiliary carry
        0xd2, 0xc4, // ROL AH,CL: changes only CF/OF when the count is nonzero
        0x0f, 0x94, 0xc3, // SETZ BL preserves ADD's zero result
        0x0f, 0x90, 0xc7, // SETO BH observes the rotate's count rule
        0x46, // INC ESI preserves the selected carry
        0x83, 0xd2, 0, // ADC EDX,0 consumes that carry
    ];
    for count in [0, 1, 8, 9, 32, 33] {
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4433_80ff;
        image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
        image.cpu.registers.edx = 0xccbb_aa01;
        image.cpu.registers.esi = 0x7fff_ffff;
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x4433_8000;
        cpu.flags.kind = 2;
        cpu.flags.left = 0xff;
        cpu.flags.right = 1;
        let mut steps = vec![retire(&mut cpu, 2, &[])];
        let add_flags = StatusFlags {
            cf: 1,
            pf: 1,
            af: 1,
            zf: 1,
            sf: 0,
            of: 0,
        };
        let rotated = rotate_expected(Operation::Rol, 8, 0x80, count, add_flags);
        cpu.registers.eax = 0x4433_0000 | (rotated.value << 8);
        rotated.apply_flags(&mut cpu);
        let flags = rotated.status.unwrap_or(add_flags);
        steps.push(retire(&mut cpu, 2, &[]));
        cpu.registers.ebx = 0x10ff_ee01;
        steps.push(retire(&mut cpu, 3, &[]));
        cpu.registers.ebx = 0x10ff_0001 | (u32::from(flags.of) << 8);
        steps.push(retire(&mut cpu, 3, &[]));
        cpu.registers.esi = 0x8000_0000;
        cpu.flags.kind = 0;
        cpu.flags.status = StatusFlags {
            cf: flags.cf,
            pf: 1,
            af: 1,
            zf: 0,
            sf: 1,
            of: 1,
        };
        steps.push(retire(&mut cpu, 1, &[]));
        cpu.registers.edx = 0xccbb_aa01 + u32::from(flags.cf);
        cpu.flags.status = StatusFlags {
            cf: 0,
            pf: 0,
            af: 0,
            zf: 0,
            sf: 1,
            of: 0,
        };
        steps.push(retire(&mut cpu, 3, &[]));
        check(
            TestModule::interpreter(),
            "ADD/rotate/INC carry reaches ADC",
            &image,
            &steps,
        );
        // The interpreter published ADD's lazy operands at its first boundary.
        // The block's final concrete ADC record leaves its incoming payload intact.
        cpu.flags.left = image.cpu.flags.left;
        cpu.flags.right = image.cpu.flags.right;
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 6).unwrap());
        check(
            &block,
            "partial flag effects compose before publication",
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn conditional_shift_and_rotate_flags_are_observed_before_a_full_replacement() {
    for shift_count in [0, 1] {
        for rotate_count in [0, 1, 8] {
            let code = [
                0xd2,
                0xe4, // SHL AH,CL
                0xb1,
                rotate_count, // MOV CL, new count
                0xd2,
                0xc8, // ROR AL,CL
                0x0f,
                0x94,
                0xc3, // SETZ BL reads a preserved flag
                0x0f,
                0x92,
                0xc7, // SETC BH reads the latest applicable carry
                0x31,
                0xd2, // XOR EDX,EDX replaces every prior effect
            ];
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4433_8001;
            image.cpu.registers.ecx = 0x8877_6600 | shift_count;
            let mut cpu = image.cpu;
            let prior = if shift_count == 0 {
                PRIOR_FLAGS
            } else {
                cpu.registers.eax = 0x4433_0001;
                cpu.flags.kind = 0;
                cpu.flags.status = StatusFlags {
                    cf: 1,
                    pf: 1,
                    af: 0,
                    zf: 1,
                    sf: 0,
                    of: 1,
                };
                cpu.flags.status
            };
            let mut steps = vec![retire(&mut cpu, 2, &[])];
            cpu.registers.ecx = 0x8877_6600 | u32::from(rotate_count);
            steps.push(retire(&mut cpu, 2, &[]));
            let rotated = rotate_expected(Operation::Ror, 8, 1, rotate_count, prior);
            cpu.registers.eax = (cpu.registers.eax & 0xffff_ff00) | rotated.value;
            rotated.apply_flags(&mut cpu);
            let flags = rotated.status.unwrap_or(prior);
            steps.push(retire(&mut cpu, 2, &[]));
            cpu.registers.ebx = 0x10ff_ee00 | u32::from(flags.zf);
            steps.push(retire(&mut cpu, 3, &[]));
            cpu.registers.ebx = 0x10ff_0000 | (u32::from(flags.cf) << 8) | u32::from(flags.zf);
            steps.push(retire(&mut cpu, 3, &[]));
            cpu.registers.edx = 0;
            cpu.flags.kind = 11;
            cpu.flags.left = 0;
            steps.push(retire(&mut cpu, 2, &[]));
            check(
                TestModule::interpreter(),
                "conditional full and partial flags feed conditions",
                &image,
                &steps,
            );
            // Only XOR's final lazy record reaches the snapshot boundary.
            cpu.flags.status = image.cpu.flags.status;
            let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 6).unwrap());
            check(
                &block,
                "full replacement discards prior conditional flag effects",
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

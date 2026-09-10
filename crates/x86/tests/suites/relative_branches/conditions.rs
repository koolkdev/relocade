use wasm86_x86::{CpuState, StatusFlags};

use crate::support::{
    arithmetic,
    machine::{both, Exit, Image, Step},
    step::TestModule,
};

fn check_conditions(name: &str, prefix: &[u8], initial: CpuState, after: CpuState, outcomes: u16) {
    for condition in 0..16 {
        for branch in [
            vec![0x70 + condition, 0x7f],
            vec![0x0f, 0x80 + condition, 0x7f, 0, 0, 0],
            vec![0x66, 0x0f, 0x80 + condition, 0x7f, 0],
        ] {
            let code = [prefix, branch.as_slice()].concat();
            let mut image = Image::new(&code);
            image.cpu = initial;
            let mut cpu = after;
            let mut steps = Vec::new();
            if !prefix.is_empty() {
                cpu.eip = 0x1000 + prefix.len() as u32;
                cpu.instruction_count = 0;
                steps.push(Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                });
            }
            cpu.eip = 0x1000 + code.len() as u32;
            if outcomes & (1 << condition) != 0 {
                cpu.eip += 0x7f;
            }
            cpu.instruction_count = u32::from(!prefix.is_empty());
            steps.push(Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            });
            both(
                TestModule::interpreter(),
                &format!("{name}, branch {branch:02x?}"),
                &code,
                steps.len() as u32,
                &image,
                &steps,
            );
        }
    }
}

#[test]
fn all_conditions_read_concrete_flags_without_changing_the_record() {
    for (name, status, outcomes) in [
        (
            "all condition inputs clear",
            StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 0,
                sf: 0,
                of: 0,
            },
            0xaaaa,
        ),
        (
            "equal without carry",
            StatusFlags {
                cf: 0,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            },
            0x665a,
        ),
        (
            "carry and overflow",
            StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 1,
            },
            0x5655,
        ),
        (
            "negative without overflow",
            StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 0,
                sf: 1,
                of: 0,
            },
            0x59aa,
        ),
    ] {
        let mut image = arithmetic::image(&[]);
        image.cpu.flags.status = status;
        check_conditions(name, &[], image.cpu, image.cpu, outcomes);
    }
}

#[test]
fn all_conditions_read_stored_lazy_flags_without_materializing_them() {
    for (name, kind, left, right, outcomes) in [
        (
            "stored ADD carry and overflow",
            10,
            0x8000_0000,
            0x8000_0000,
            0x5655,
        ),
        (
            "stored SUB signed and unsigned disagreement",
            9,
            0x7fff_fffe,
            0xffff_fffe,
            0xa565,
        ),
        ("stored logical odd result", 11, 1, 0x1234_5678, 0xaaaa),
        ("stored logical zero result", 11, 0, 0x1234_5678, 0x665a),
    ] {
        let mut image = arithmetic::image(&[]);
        image.cpu.flags.kind = kind;
        image.cpu.flags.left = left;
        image.cpu.flags.right = right;
        check_conditions(name, &[], image.cpu, image.cpu, outcomes);
    }
}

#[test]
fn all_conditions_use_flags_created_inside_the_snapshot_block() {
    let mut image = arithmetic::image(&[]);
    image.cpu.registers.eax = 0x8000_0000;
    let mut after = image.cpu;
    after.registers.eax = 0;
    after.flags.kind = 10;
    after.flags.left = 0x8000_0000;
    after.flags.right = 0x8000_0000;
    check_conditions("local ADD", &[0x01, 0xc0], image.cpu, after, 0x5655);

    let mut image = arithmetic::image(&[]);
    image.cpu.registers.eax = 0x7fff_fffe;
    image.cpu.registers.ebx = 0xffff_fffe;
    let mut after = image.cpu;
    after.flags.kind = 9;
    after.flags.left = 0x7fff_fffe;
    after.flags.right = 0xffff_fffe;
    check_conditions("local CMP", &[0x39, 0xd8], image.cpu, after, 0xa565);

    for (result, outcomes) in [(1, 0xaaaa), (0, 0x665a)] {
        let mut image = arithmetic::image(&[]);
        image.cpu.registers.eax = result;
        let mut after = image.cpu;
        after.flags.kind = 11;
        after.flags.left = result;
        check_conditions("local TEST", &[0x85, 0xc0], image.cpu, after, outcomes);
    }

    let mut image = arithmetic::image(&[]);
    image.cpu.registers.eax = 0x7fff_ffff;
    image.cpu.flags.status.cf = 1;
    let mut after = image.cpu;
    after.registers.eax = 0x8000_0000;
    after.flags.status = StatusFlags {
        cf: 0,
        pf: 1,
        af: 1,
        zf: 0,
        sf: 1,
        of: 1,
    };
    check_conditions(
        "local ADC explicit flags",
        &[0x83, 0xd0, 0],
        image.cpu,
        after,
        0xa5a9,
    );
}

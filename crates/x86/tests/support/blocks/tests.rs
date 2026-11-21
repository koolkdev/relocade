use crate::support::config::{FLAT32, SEGMENTED32};
use relocade_x86::CpuState;
use std::panic::{catch_unwind, AssertUnwindSafe};

use super::BlockModules;

#[test]
fn cache_hits_revalidate_cs_even_when_entry_assumptions_stay_compatible() {
    let config = SEGMENTED32;
    let mut cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    cpu.segments.cs.limit = 0x1001;
    let mut blocks = BlockModules::default();
    blocks.get(&cpu, &[0xb0, 7], 1, config);
    let valid = cpu;
    for cs in [
        relocade_x86::SegmentRegister {
            limit: 0x1000,
            ..valid.segments.cs
        },
        relocade_x86::SegmentRegister {
            usable: 0,
            ..valid.segments.cs
        },
    ] {
        cpu.segments.cs = cs;
        assert!(cpu.segments.is_compatible_with(config.entry));
        assert!(catch_unwind(AssertUnwindSafe(|| {
            blocks.get(&cpu, &[0xb0, 7], 1, config);
        }))
        .is_err());
    }
    blocks.get(&valid, &[0xb0, 7], 1, config);
}

#[test]
fn admission_checks_only_instructions_inside_the_compilation_boundary() {
    let mut cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    let mut blocks = BlockModules::default();
    for (bytes, limit, cs_limit) in [
        (&[0x90, 0x0f][..], 1, 0x1000),
        (&[0xeb, 0, 0x0f][..], 3, 0x1001),
        (&[0xf3, 0xa4, 0x0f][..], 1, 0x1001),
    ] {
        cpu.segments.cs.limit = cs_limit;
        blocks.get(&cpu, bytes, limit, SEGMENTED32);
    }
    assert!(catch_unwind(AssertUnwindSafe(|| {
        blocks.get(&cpu, &[0xf3, 0xa4, 0x90], 2, SEGMENTED32);
    }))
    .is_err());
    cpu.segments.cs.limit = 0x1002;
    blocks.get(&cpu, &[0xf3, 0xa4, 0x90], 2, SEGMENTED32);
}

#[test]
fn cached_blocks_keep_their_cpu_model() {
    use crate::support::machine::{expected, Exit, Image, Step};
    use relocade_x86::{Compiler, CpuFeature, CpuModel};

    let code = [0x0f, 0x44, 0xc3]; // CMOVE EAX, EBX
    let mut image = Image::new(&code);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.zf = 1;
    let mut blocks = BlockModules::default();
    for enabled in [true, false, true] {
        let config = Compiler::from_config(FLAT32)
            .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, enabled))
            .config();
        let module = blocks.get(&image.cpu, &code, 1, config);
        let mut cpu = image.cpu;
        let exit = if enabled {
            cpu.registers.eax = cpu.registers.ebx;
            cpu.eip += 3;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            Exit::Dispatch(cpu.eip)
        } else {
            Exit::InvalidOpcode
        };
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit
                }]
            )
        );
    }
}

//! MASKMOV uses DS with overrides, an address-sized index, and no index update.

use super::*;
use wasm86_x86::{ExecutionProfile, Segments};

fn addressing(frontend: Frontend) {
    for profile in [SegmentProfile::Segmented32.into(), ExecutionProfile::Real16] {
        let mut checks = ImageSequences::new(frontend, profile);
        for bytes in [8, 16] {
            for fs in [false, true] {
                let mut prefixes = vec![];
                if profile != ExecutionProfile::Real16 {
                    prefixes.push(0x67);
                }
                if fs {
                    prefixes.push(0x64);
                }
                let code = [prefixes, code(bytes)].concat();
                let mut image = image(&code, [0x80; 16]);
                if profile == ExecutionProfile::Real16 {
                    image.cpu.segments = Segments::real_mode();
                }
                image.cpu.segments.ds.selector = 0x400;
                image.cpu.segments.ds.base = 0x4000;
                image.cpu.segments.fs.selector = 0x500;
                image.cpu.segments.fs.base = 0x5000;
                image.cpu.segments.es.selector = 0x600;
                image.cpu.segments.es.base = 0x6000;
                image.cpu.registers.edi = 0xabcd_0013;
                image.cpu.flags.bytes.df = 1;
                let page = if fs { 5 } else { 4 };
                image.map(page, 0x8000, true);
                let cpu = retired(image.cpu, code.len(), bytes);
                checks.check(
                    "MASKMOV wraps DI and selects DS or its override, independently of ES and DF",
                    &code,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[(0x8013, &DATA[..bytes])],
                        exit: Exit::Dispatch(cpu.eip),
                    }],
                );
            }
        }
    }

    let mut checks = ImageSequences::new(frontend, SegmentProfile::Segmented32);
    for bytes in [8, 16] {
        for ss in [false, true] {
            let code = [if ss { vec![0x36] } else { vec![] }, code(bytes)].concat();
            let mut image = image(&code, [0; 16]);
            image.cpu.registers.edi = 0x1_ffff;
            image.cpu.segments.ds.limit = 0xffff;
            image.cpu.segments.ss.limit = 0xffff;
            image.cpu.segments.ss.big = 0;
            checks.check(
                "full EDI and segment faults apply even to an empty mask",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: if ss {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    },
                }],
            );
        }
    }
}

test_frontends!(masked_addressing, addressing);

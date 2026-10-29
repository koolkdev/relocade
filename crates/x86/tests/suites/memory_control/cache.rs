//! Prefetch is a non-faulting hint; CLFLUSH checks one byte without loading data.

use super::*;
use crate::support::{
    blocks::BlockModules,
    encoding::check_length,
    machine::{expected, retire},
    step::TestModule,
};
use relocade_x86::SegmentRegister;

fn prefetch(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Segmented32);
    for hint in 0..4 {
        let code = [0x64, 0x66, 0x0f, 0x18, hint << 3];
        let mut image = Image::new(&code);
        image.cpu.registers.eax = u32::MAX;
        image.cpu.segments.fs.usable = 0;
        image.cpu.segments.fs.limit = 0;
        let cpu = retire(image.cpu, code.len());
        checks.check(
            "prefetch ignores unusable segments, out-of-range offsets and absent pages",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn flush(frontend: Frontend) {
    let code = [0x0f, 0xae, 0x38]; // CLFLUSH [eax]
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for present in [false, true] {
        let mut image = Image::new(&code);
        image.cpu.registers.eax = 0x4fff;
        if present {
            image.map(4, 0x8000, false);
        }
        let cpu = if present {
            retire(image.cpu, code.len())
        } else {
            image.cpu
        };
        checks.check(
            "CLFLUSH requires only a readable byte, with no alignment or write permission",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: if present {
                    Exit::Dispatch(cpu.eip)
                } else {
                    Exit::PageFault {
                        address: 0x4fff,
                        error: 0,
                    }
                },
            }],
        );
    }

    let code = [0xc6, 0x00, 0x5a, 0x0f, 0xae, 0x39]; // MOV byte [eax],5a; CLFLUSH [ecx]
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0x4000;
    image.cpu.registers.ecx = 0x5000;
    image.map(4, 0x8000, true);
    let cpu = retire(image.cpu, 3);
    checks.check(
        "a CLFLUSH page fault preserves an earlier completed store",
        &code,
        &image,
        &[
            Step {
                cpu,
                ram: &[(0x8000, &[0x5a])],
                exit: Exit::Dispatch(cpu.eip),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            },
        ],
    );
}

fn segment_permissions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Segmented32);
    for (cache, limit, allowed) in [
        (
            SegmentRegister {
                readable: 0,
                ..SegmentRegister::flat_code32(0)
            },
            0xfff,
            true,
        ),
        (
            SegmentRegister {
                writable: 0,
                ..SegmentRegister::flat_data32(0)
            },
            0xfff,
            true,
        ),
        (SegmentRegister::unusable(0), 0xfff, false),
        (
            SegmentRegister {
                readable: 0,
                ..SegmentRegister::flat_code32(0)
            },
            0xffe,
            false,
        ),
    ] {
        let code = [0x64, 0x0f, 0xae, 0x38];
        let mut image = Image::new(&code);
        image.cpu.segments.fs = cache;
        image.cpu.segments.fs.limit = limit;
        image.cpu.segments.fs.base = 0x4000;
        image.cpu.registers.eax = 0xfff;
        image.map(4, 0x8000, false);
        let cpu = if allowed {
            retire(image.cpu, code.len())
        } else {
            image.cpu
        };
        checks.check(
            "CLFLUSH permits execute-only code but retains segment usability and bounds checks",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: if allowed {
                    Exit::Dispatch(cpu.eip)
                } else {
                    Exit::GeneralProtection { error: 0 }
                },
            }],
        );
    }
    let code = [0x36, 0x0f, 0xae, 0x38];
    let mut image = Image::new(&code);
    image.cpu.segments.ss.limit = 0xfff;
    image.cpu.registers.eax = 0x1000;
    checks.check(
        "CLFLUSH retains stack-segment fault classification",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::StackFault { error: 0 },
        }],
    );
}

fn physical_addresses(frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for flush in [false, true] {
        let code = [
            0x67,
            0x0f,
            if flush { 0xae } else { 0x18 },
            if flush { 0x38 } else { 0 },
        ];
        for address in [0x4011, 0x1_0000] {
            let mut image = Image::new(&code);
            image.cpu.segments = Segments::real_mode();
            image.cpu.registers.eax = address;
            let mut input = image.input();
            input.mmio_pages = vec![(4, 0x8000)];
            input.observe_mmio = true;
            let denied = flush && address > 0xffff;
            let cpu = if denied {
                image.cpu
            } else {
                retire(image.cpu, code.len())
            };
            let wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: if denied {
                        Exit::GeneralProtection { error: 0 }
                    } else {
                        Exit::Dispatch(cpu.eip)
                    },
                }],
            );
            let module = match frontend {
                Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
                Frontend::Interpreter => {
                    TestModule::interpreter_with_profile(ExecutionProfile::Real16)
                }
            };
            assert_eq!(
                module.observe(&input, 1),
                wanted,
                "cache operations must not call MMIO"
            );
        }
    }
}

test_frontends!(prefetch_hints, prefetch);
test_frontends!(cache_flush_checks, flush);
test_frontends!(cache_flush_segments, segment_permissions);
test_frontends!(cache_physical_addresses, physical_addresses);

#[test]
fn cache_instruction_encodings() {
    check_length(&[0x0f, 0x18, 0x84, 0x8d, 0x78, 0x56, 0x34, 0x12]);
    for hint in 0..4 {
        check_unsupported_at_page_end(&[0x0f, 0x18, 0xc0 | hint << 3], 0x0f);
    }
    for code in [
        &[0xf0, 0x0f, 0x18, 0][..],
        &[0xf0, 0x0f, 0xae, 0x38],
        &[0xf2, 0x0f, 0xae, 0x38],
        &[0xf3, 0x0f, 0xae, 0x38],
    ] {
        check_unsupported_at_page_end(code, code[0]);
    }
    // The 66 form is CLFLUSHOPT, outside the P4 target.
    check_unsupported_at_page_end(&[0x66, 0x0f, 0xae, 0x38], 0x0f);
}

#[test]
fn prefetch_requires_complete_instruction_fetch() {
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ffc;
    image.data(0x3ffc, &[0x0f, 0x18, 0x84, 0x8d]);
    image.check_unchanged_exit(
        TestModule::interpreter(),
        "a non-faulting prefetch still fetches its complete displacement",
        Exit::PageFault {
            address: 0x2000,
            error: 16,
        },
    );
}

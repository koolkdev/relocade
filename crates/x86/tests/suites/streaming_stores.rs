//! Streaming stores retain payloads, access checks and MMX state transitions.

use crate::support::{
    encoding::check_unsupported_at_page_end,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{stack_image, write_register_bits},
};
use wasm86_x86::{ExecutionProfile, SegmentProfile, Segments};

const BITS: u128 = 0x7f800001_80000000_01234567_89abcdef;
const FORMS: &[(&[u8], usize, bool)] = &[
    (&[0x0f, 0x2b, 0x30], 16, false), // [eax], xmm6
    (&[0x66, 0x0f, 0x2b, 0x30], 16, false),
    (&[0x66, 0x0f, 0xe7, 0x30], 16, false),
    (&[0x0f, 0xe7, 0x30], 8, true),  // [eax], mm6
    (&[0x0f, 0xc3, 0x30], 4, false), // [eax], esi
];

fn stores(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for &(code, bytes, mmx) in FORMS {
        let offsets: &[u32] = if bytes == 16 {
            &[0x4ff0]
        } else {
            &[0x4ff0, 0x4ffd]
        };
        for &address in offsets {
            let mut image = stack_image(code, 5, 0xaaaa);
            image.cpu.registers.eax = address;
            image.cpu.registers.esi = BITS as u32;
            image.cpu.simd.xmm[6] = BITS.to_le_bytes();
            image.cpu.simd.mxcsr = 0x9fc5;
            write_register_bits(&mut image.cpu, 6, (BITS as u64, 0x1234));
            image.map(4, 0x8000, true);
            image.map(5, 0xc000, true);
            image.data(0x8ff0, &[0x5a; 16]);
            image.data(0xc000, &[0x5a; 16]);
            let mut cpu = image.cpu;
            cpu.eip += code.len() as u32;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            if mmx {
                cpu.x87.tag_word = 0;
                cpu.x87.status.top = 0;
            }
            let payload = BITS.to_le_bytes();
            let first = bytes.min((0x5000 - address) as usize);
            let mut writes = vec![(address + 0x4000, &payload[..first])];
            if first < bytes {
                writes.push((0xc000, &payload[first..bytes]));
            }
            checks.check(
                "streaming stores preserve source state and use their exact width",
                code,
                &image,
                &[Step {
                    cpu,
                    ram: &writes,
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for &(code, bytes, mmx) in FORMS {
        for writable in [false, true] {
            let mut image = stack_image(code, 5, 0xaaaa);
            image.cpu.registers.eax = if bytes == 16 { 0x5000 } else { 0x4ffe };
            image.map(4, 0x8000, writable);
            image.data(0x8ffe, &[0x5a; 2]);
            let (address, error) = if bytes != 16 && !writable {
                (0x4ffe, 3)
            } else {
                (0x5000, 2)
            };
            checks.check(
                "denied streaming stores leave memory and MMX state unchanged",
                code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault { address, error },
                }],
            );
        }
        if bytes == 16 {
            let mut image = stack_image(code, 5, 0xaaaa);
            image.cpu.registers.eax = 0x4001;
            checks.check(
                "streaming XMM alignment faults precede paging",
                code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            );
        }
        if mmx {
            let mut image = stack_image(code, 5, 0xaaaa);
            image.cpu.x87.status.error_summary = 1;
            image.cpu.registers.eax = 0x4000;
            checks.check(
                "MOVNTQ admits pending x87 exceptions before destination access",
                code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }],
            );
        }
    }
}

fn real_mode(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, ExecutionProfile::Real16);
    for &(form, bytes, mmx) in FORMS {
        // The address override chooses EAX, but MOVNTI remains a dword store
        // under a 16-bit code default, and P66 still selects the XMM variant.
        let code = [&[0x67][..], form].concat();
        let mut image = stack_image(&code, 5, 0xaaaa);
        image.cpu.segments = Segments::real_mode();
        image.cpu.segments.ds.selector = 0x400;
        image.cpu.segments.ds.base = 0x4000;
        image.cpu.registers.eax = 0x10;
        image.cpu.registers.esi = BITS as u32;
        image.cpu.simd.xmm[6] = BITS.to_le_bytes();
        write_register_bits(&mut image.cpu, 6, (BITS as u64, 0x1234));
        image.map(4, 0x8000, true);
        let mut cpu = image.cpu;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        if mmx {
            cpu.x87.tag_word = 0;
            cpu.x87.status.top = 0;
        }
        checks.check(
            "streaming stores retain widths under a 16-bit code default",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8010, &BITS.to_le_bytes()[..bytes])],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

test_frontends!(streaming_payloads, stores);
test_frontends!(streaming_faults, faults);
test_frontends!(streaming_real_mode, real_mode);

#[test]
fn streaming_stores_require_memory_destinations() {
    for &(form, _, _) in FORMS {
        let mut code = form.to_vec();
        *code.last_mut().unwrap() |= 0xc0;
        check_unsupported_at_page_end(&code, 0x0f);
    }
}

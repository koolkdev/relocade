use crate::support::{
    encoding::{check_length, check_unsupported_at_page_end},
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
};
use relocade_x86::{
    DescriptorTableRegister, ExecutionProfile, Segment, SegmentProfile, SegmentRegister,
};

fn store_formats(frontend: Frontend) {
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        let mut checks = ImageSequences::new(frontend, profile);
        for operand_override in [false, true] {
            let real = profile == ExecutionProfile::Real16;
            let mut code = if operand_override { vec![0x66] } else { vec![] };
            code.extend_from_slice(if real {
                &[0x0f, 0x01, 0x46, 0]
            } else {
                &[0x0f, 0x01, 0x03]
            });
            let mut image = Image::new(&code);
            if real {
                image.initialize_real_mode();
                image.cpu.segments.ss = SegmentRegister::real_mode(Segment::Ss, 0x400);
                image.cpu.registers.ebp = 0xaaaa_0020; // Fractint uses SGDT [BP].
            } else {
                image.cpu.registers.ebx = 0x4020;
            }
            image.cpu.gdtr = DescriptorTableRegister::new(0x89ab_cdef, 0x1234);
            image.cpu.gdtr.reserved = [0xff; 2];
            image.map(4, 0x8000, true);
            image.data(0x801f, &[0xa5; 8]);
            let mut cpu = image.cpu;
            cpu.eip += code.len() as u32;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            let high = if real != operand_override { 0 } else { 0x89 };
            checks.check(
                "six-byte output uses operand size and preserves the CPU and adjacent bytes",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x8020, &[0x34, 0x12, 0xef, 0xcd, 0xab, high])],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }

    let code = [0x66, 0x67, 0x64, 0x0f, 0x01, 0x03]; // Real mode, dword, FS:[EBX].
    let mut image = Image::new(&code);
    image.initialize_real_mode();
    image.cpu.segments.fs = SegmentRegister::real_mode(Segment::Fs, 0x500);
    image.cpu.registers.ebx = 0x20;
    image.cpu.gdtr = DescriptorTableRegister::new(0x1234_5678, 0xabcd);
    image.map(5, 0x9000, true);
    let mut cpu = image.cpu;
    cpu.eip += code.len() as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    ImageSequences::new(frontend, ExecutionProfile::Real16).check(
        "address and segment overrides select the destination independently of operand size",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[(0x9020, &[0xcd, 0xab, 0x78, 0x56, 0x34, 0x12])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
test_frontends!(stores_gdtr, store_formats);

fn destination_pages(frontend: Frontend) {
    let code = [0x0f, 0x01, 0x03];
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for second_page in [None, Some(false), Some(true)] {
        let mut image = Image::new(&code);
        image.cpu.gdtr = DescriptorTableRegister::new(0x89ab_cdef, 0x1234);
        image.cpu.registers.ebx = 0x4ffd;
        image.map(4, 0x8000, true);
        image.data(0x8ffc, &[0xa5; 4]);
        image.data(0xa000, &[0xa5; 4]);
        if let Some(writable) = second_page {
            image.map(5, 0xa000, writable);
        }
        let mut cpu = image.cpu;
        let ram: &[(u32, &[u8])] = if second_page == Some(true) {
            cpu.eip += code.len() as u32;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            &[(0x8ffd, &[0x34, 0x12, 0xef]), (0xa000, &[0xcd, 0xab, 0x89])]
        } else {
            &[]
        };
        let exit = match second_page {
            Some(true) => Exit::Dispatch(cpu.eip),
            Some(false) => Exit::PageFault {
                address: 0x5000,
                error: 3,
            },
            None => Exit::PageFault {
                address: 0x5000,
                error: 2,
            },
        };
        checks.check(
            "check the complete six-byte destination before storing",
            &code,
            &image,
            &[Step { cpu, ram, exit }],
        );
    }
}
test_frontends!(checks_both_pages, destination_pages);

fn destination_segments(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Segmented32);
    for (stack, writable, offset) in [
        (false, true, 0x4ffa),
        (false, true, 0x4ffb),
        (true, true, 0x4ffb),
        (false, false, 0x4ffa),
    ] {
        let mut code = if stack { vec![0x36] } else { vec![] };
        code.extend_from_slice(&[0x0f, 0x01, 0x03]);
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = offset;
        image.cpu.gdtr = DescriptorTableRegister::new(0x89ab_cdef, 0x1234);
        let segment = if stack {
            &mut image.cpu.segments.ss
        } else {
            &mut image.cpu.segments.ds
        };
        segment.limit = 0x4fff;
        segment.writable = u8::from(writable);
        image.map(4, 0x8000, true);
        image.data(0x8ffa, &[0xa5; 6]);
        let mut cpu = image.cpu;
        let success = writable && offset == 0x4ffa;
        let ram: &[(u32, &[u8])] = if success {
            cpu.eip += code.len() as u32;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            &[(0x8ffa, &[0x34, 0x12, 0xef, 0xcd, 0xab, 0x89])]
        } else {
            &[]
        };
        let exit = if success {
            Exit::Dispatch(cpu.eip)
        } else if stack {
            Exit::StackFault { error: 0 }
        } else {
            Exit::GeneralProtection { error: 0 }
        };
        checks.check(
            "segment checks cover all six bytes before page checks",
            &code,
            &image,
            &[Step { cpu, ram, exit }],
        );
    }
}
test_frontends!(checks_segment_span, destination_segments);

#[test]
fn encoding_requires_memory_and_rejects_lock() {
    check_length(&[0x0f, 0x01, 0x44, 0x8b, 0x7f]);
    check_unsupported_at_page_end(&[0x0f, 0x01, 0xc0], 0x0f);
    check_unsupported_at_page_end(&[0xf0, 0x0f, 0x01, 0x03], 0xf0);
}

use super::*;

fn registers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for index in [0, 1, 2, 3, 4, 5, 6, 7, 0xf9, 0xff] {
        let code = [0x66, 0x0f, 0xc5, 0xc6, index, 0x66, 0x0f, 0xc4, 0xf1, index]; // eax,xmm6; xmm6,ecx
        let mut image = image(&code);
        image.cpu.registers.eax = u32::MAX;
        image.cpu.registers.ecx = 0xdeadbeef;
        image.cpu.x87.status.error_summary = 1;
        let offset = usize::from(index & 7) * 2;
        let bytes = &image.cpu.simd.xmm[6];
        let mut extracted = retire(image.cpu, 5);
        extracted.registers.eax = u32::from(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]));
        let mut inserted = retire(extracted, 5);
        inserted.simd.xmm[6][offset..offset + 2].copy_from_slice(&0xbeef_u16.to_le_bytes());
        checks.check(
            "word index ignores upper immediate bits and preserves other lanes",
            &code,
            &image,
            &[dispatch(extracted), dispatch(inserted)],
        );
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0x66, 0x0f, 0xc4, 0x30, 0xff]; // xmm6,[eax],7
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ffe;
    image.map(4, 0x8000, false);
    image.data(0x8ffe, &[0xab, 0xcd]);
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[6][14..16].copy_from_slice(&[0xab, 0xcd]);
    checks.check(
        "PINSRW reads exactly two bytes",
        &code,
        &image,
        &[dispatch(cpu)],
    );
    image.cpu.registers.eax = 0x4fff;
    checks.check(
        "word read fault preserves the whole destination",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        }],
    );
}

test_frontends!(sse_word_registers, registers);
test_frontends!(sse_word_memory, memory);

fn segmented(engine: Engine, frontend: Frontend) {
    use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented16);
    for mmx in [false, true] {
        let mut code = if mmx { vec![] } else { vec![0x66] };
        code.extend_from_slice(&[0x0f, 0xc5, 0xc7, 0xff]);
        let extract_length = code.len();
        if !mmx {
            code.push(0x66);
        }
        code.extend_from_slice(&[0x0f, 0xc4, 0x3e, 0xfe, 0x4f, 0xff]); // mm/xmm7,[0x4ffe],ff
        let mut image = if mmx {
            crate::support::x87::stack_image(&code, 5, 0xffff)
        } else {
            image(&code)
        };
        image.cpu.segments.cs.attributes = SegmentAttributes::new(
            SegmentKind::Code { readable: true },
            SegmentDefaultSize::Bits16,
        );
        image.cpu.registers.eax = u32::MAX;
        image.cpu.simd.xmm[7] = [0x80; 16];
        image.cpu.x87.registers[7].significand = 0x8080000000000000;
        image.map(4, 0x8000, false);
        image.data(0x8ffe, &[0xef, 0xbe]);
        let mut cpu = retire(image.cpu, extract_length);
        cpu.registers.eax = 0x8080;
        if mmx {
            cpu.x87.tag_word = 0;
            cpu.x87.status.top = 0;
        }
        let extracted = cpu;
        cpu = retire(cpu, code.len() - extract_length);
        if mmx {
            cpu.x87.registers[7].significand = 0xbeef000000000000;
            cpu.x87.registers[7].sign_exponent = 0xffff;
        } else {
            cpu.simd.xmm[7][14..].copy_from_slice(&[0xef, 0xbe]);
        }
        checks.check(
            "PEXTRW writes a dword and PINSRW reads a word in 16-bit code",
            &code,
            &image,
            &[dispatch(extracted), dispatch(cpu)],
        );
    }
}

test_frontends!(simd_word_transfers_in_16_bit_code, segmented);

#[test]
fn simd_word_extraction_rejects_memory() {
    for code in [&[0x0f, 0xc5, 0x04][..], &[0x66, 0x0f, 0xc5, 0x04]] {
        crate::support::encoding::check_unsupported_at_page_end(code, 0x0f);
    }
}

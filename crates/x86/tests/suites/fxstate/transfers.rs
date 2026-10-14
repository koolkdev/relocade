//! Representative addressing and page-routing integration for the FXSR image.

use super::*;

fn split_pages(engine: Engine, frontend: Frontend) {
    for restore in [false, true] {
        let code = instruction(FLAT32, restore, 0x4f70);
        let mut image = image(&code, FLAT32);
        image.map(5, 0xc000, true);
        image.data(0x8f70, &[0xa5; 144]);
        image.data(0xc000, &[0xa5; 368]);
        let bytes = saved_bytes(
            &image.cpu,
            [
                0x2800_037f,
                0x0654_008f,
                0x1234_5678,
                0x17,
                0x89ab_cdef,
                0x27,
                0x9fc5,
                0xffff,
            ],
            [5, 6, 7, 0, 1, 2, 3, 4],
        );
        let mut cpu = retire(image.cpu, code.len());
        let ram = if restore {
            image.data(0x8f70, &bytes[..144]);
            image.data(0xc000, &bytes[144..]);
            cpu.x87.tag_word = 0x3f00; // Every occupied payload in stack_image is normal.
            cpu.simd.mxcsr = 0x9fc5;
            vec![]
        } else {
            vec![(0x8f70, &bytes[..144]), (0xc000, &bytes[144..])]
        };
        ImageSequences::new(engine, frontend, FLAT32).check(
            "FXSR transfers an image across noncontiguous physical frames",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &ram,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn address_override(engine: Engine, frontend: Frontend) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Segmented16);
    for restore in [false, true] {
        let code = [&[0x67][..], &instruction(FLAT32, restore, 0x14010)].concat();
        let mut image = image(&code, profile);
        image.map(4, 0xc000, true);
        image.data(0xc010, &[0xcc; 512]);
        image.map(0x14, 0x8000, true);
        let bytes = saved_bytes(
            &image.cpu,
            [
                0x2800_037f,
                0x0654_008f,
                0x5678,
                0x17,
                0xcdef,
                0x27,
                0x9fc5,
                0xffff,
            ],
            [5, 6, 7, 0, 1, 2, 3, 4],
        );
        let mut cpu = retire(image.cpu, code.len());
        let ram = if restore {
            let mut source = bytes;
            source[8..12].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
            source[16..20].copy_from_slice(&0x89ab_cdef_u32.to_le_bytes());
            image.data(0x8010, &source);
            cpu.x87.instruction_offset = 0x5678;
            cpu.x87.data_offset = 0xcdef;
            cpu.x87.tag_word = 0x3f00;
            cpu.simd.mxcsr = 0x9fc5;
            vec![]
        } else {
            vec![(0x8010, bytes.as_slice())]
        };
        ImageSequences::new(engine, frontend, profile).check(
            "FXSR address override changes addressing while pointer fields remain 16-bit",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &ram,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

test_frontends!(fxstate_split_pages, split_pages);
test_frontends!(fxstate_address_override, address_override);

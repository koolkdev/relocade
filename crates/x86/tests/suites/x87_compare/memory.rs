//! Real-memory comparisons retain source exception evidence and access boundaries.

use super::*;

fn instruction(opcode: u8, pop: bool, address: u32) -> Vec<u8> {
    [
        vec![opcode, if pop { 0x1d } else { 0x15 }],
        address.to_le_bytes().to_vec(),
    ]
    .concat()
}

fn memory_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, bytes) in [
        (0xd8, 0x4000_0000_u32.to_le_bytes().to_vec()),
        (0xdc, 0x4000_0000_0000_0000_u64.to_le_bytes().to_vec()),
    ] {
        for pop in [false, true] {
            // An operand ends at the last mapped byte. 66 cannot change its width.
            let address = 0x5000 - bytes.len() as u32;
            let code = [vec![0x66], instruction(opcode, pop, address)].concat();
            let mut image = initial_image(&code);
            write_value(&mut image.cpu, 7, ONE);
            image.map(4, 0x8000, false);
            image.data(address + 0x4000, &bytes);
            let saved_opcode = (u16::from(opcode & 7) << 8) | u16::from(code[2]);
            let mut result = completed(image.cpu, 7, saved_opcode, LESS, u8::from(pop));
            result.x87.data_offset = address;
            result.x87.data_selector = 0x23;
            checks.check(
                "real-memory form and fixed source width",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

struct MemoryCase {
    opcode: u8,
    bytes: Vec<u8>,
    left: (u64, u16),
    control: u16,
    flags: u16,
    empty: bool,
    pops: u8,
}

fn memory_operands(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for MemoryCase {
        opcode,
        bytes,
        left,
        flags,
        control,
        empty,
        pops,
    } in [
        MemoryCase {
            opcode: 0xd8,
            bytes: 0x3f80_0000_u32.to_le_bytes().to_vec(),
            left: (LEADING + 1, 0x3fff),
            flags: 0,
            control: 0x007f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 0x3ff0_0000_0000_0001_u64.to_le_bytes().to_vec(),
            left: (LEADING + 1, 0x3fff),
            flags: LESS,
            control: 0x007f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 1_u32.to_le_bytes().to_vec(),
            left: (LEADING, 0x3f6a),
            flags: EQUAL | 2,
            control: 0x037f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 1_u64.to_le_bytes().to_vec(),
            left: (LEADING, 0x3bcd),
            flags: EQUAL | 2,
            control: 0x037f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 1_u32.to_le_bytes().to_vec(),
            left: ONE,
            flags: 0x4102 | PENDING,
            control: 0x037d,
            empty: false,
            pops: 0,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 1_u64.to_le_bytes().to_vec(),
            left: ONE,
            flags: 0x4102 | PENDING,
            control: 0x037d,
            empty: false,
            pops: 0,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 0x7fc0_0001_u32.to_le_bytes().to_vec(),
            left: ONE,
            flags: UNORDERED | 1,
            control: 0x037f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 0x7ff8_0000_0000_0001_u64.to_le_bytes().to_vec(),
            left: ONE,
            flags: 0x4101 | PENDING,
            control: 0x037e,
            empty: false,
            pops: 0,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 0x7f80_0001_u32.to_le_bytes().to_vec(),
            left: ONE,
            flags: 0x4101 | PENDING,
            control: 0x037e,
            empty: false,
            pops: 0,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 0x7ff0_0000_0000_0001_u64.to_le_bytes().to_vec(),
            left: ONE,
            flags: UNORDERED | 1,
            control: 0x037f,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 1_u32.to_le_bytes().to_vec(),
            left: QNAN,
            flags: UNORDERED | 1,
            control: 0x037d,
            empty: false,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xdc,
            bytes: 1_u64.to_le_bytes().to_vec(),
            left: ONE,
            flags: UNORDERED | 0x41,
            control: 0x037d,
            empty: true,
            pops: 1,
        },
        MemoryCase {
            opcode: 0xd8,
            bytes: 1_u32.to_le_bytes().to_vec(),
            left: ONE,
            flags: 0x4141 | PENDING,
            control: 0x037e,
            empty: true,
            pops: 0,
        },
    ] {
        let code = instruction(opcode, true, 0x4000);
        let mut image = initial_image(&code);
        set_control(&mut image.cpu.x87.control, control);
        write_value(&mut image.cpu, 7, left);
        if empty {
            image.cpu.x87.tag_word |= 0xc000;
        }
        image.map(4, 0x8000, false);
        image.data(0x8000, &bytes);
        let mut result = completed(
            image.cpu,
            6,
            (u16::from(opcode & 7) << 8) | 0x1d,
            flags,
            pops,
        );
        result.x87.data_offset = 0x4000;
        result.x87.data_selector = 0x23;
        checks.check(
            &format!("memory source {bytes:02x?}, control {control:04x}, empty {empty}"),
            &code,
            &image,
            &[dispatch(result)],
        );
    }
}

fn fault_ordering(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = instruction(0xdc, true, 0x4ffc);
    let mut image = initial_image(&code);
    image.cpu.x87.tag_word |= 0xc000;
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.map(4, 0x8000, false);
    image.data(0x8ffc, &[1, 0, 0, 0]);
    checks.check(
        "split source fault precedes stack and status effects",
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
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception precedes comparison memory access",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(forms, memory_forms);
test_frontends!(operands, memory_operands);
test_frontends!(faults, fault_ordering);

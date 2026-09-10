use wasm86_x86::{compile_block_from_bytes, Gpr32};

use crate::support::machine::{both, byte_register_image, expected, Exit, Image, Step};
use crate::support::step::TestModule;

#[test]
fn readonly_sources_need_only_their_encoded_width() {
    let step = TestModule::interpreter();
    for (opcode, word_destination, address, eax) in [
        (0xb6, false, 0x4fff, 0x0000_0080),
        (0xbe, false, 0x4fff, 0xffff_ff80),
        (0xb6, true, 0x4fff, 0x4433_0080),
        (0xbe, true, 0x4fff, 0x4433_ff80),
        (0xb7, false, 0x4ffe, 0x0000_80a1),
        (0xbf, false, 0x4ffe, 0xffff_80a1),
        (0xb7, true, 0x4ffe, 0x4433_80a1),
        (0xbf, true, 0x4ffe, 0x4433_80a1),
    ] {
        let mut code = if word_destination { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[0x0f, opcode, 0x03]);
        let mut image = byte_register_image(&code);
        image.cpu.registers.ebx = address;
        image.map(4, 0x8000, false);
        image.data(0x8ffd, &[0x5a, 0xa1, 0x80]);
        let mut expected = image.cpu;
        expected.registers.eax = eax;
        expected.eip += code.len() as u32;
        expected.instruction_count = 0;
        both(
            step,
            &format!("read-only source at {address:04x} via {code:02x?}"),
            &code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(expected.eip),
            }],
        );
    }
}

#[test]
fn word_sources_cross_contiguous_and_scattered_pages() {
    let step = TestModule::interpreter();
    for (opcode, frame, eax) in [(0xb7, 0x9000, 0x0000_80a1), (0xbf, 0xa000, 0xffff_80a1)] {
        let code = [0x0f, opcode, 0x03];
        let mut image = byte_register_image(&code);
        image.cpu.registers.ebx = 0x4fff;
        image.map(4, 0x8000, false);
        image.map(5, frame, false);
        image.data(0x8ffe, &[0x5a, 0xa1]);
        image.data(frame, &[0x80, 0x5a]);
        let mut expected = image.cpu;
        expected.registers.eax = eax;
        expected.eip = 0x1003;
        expected.instruction_count = 0;
        both(
            step,
            "word source spans two translated pages",
            &code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(0x1003),
            }],
        );
    }
}

#[test]
fn effective_addresses_use_the_old_full_destination() {
    let step = TestModule::interpreter();
    for (name, code, old_eax, eax) in [
        (
            "word destination preserves the high address bits",
            &[0x66, 0x0f, 0xbe, 0x00][..],
            0x8000_4010,
            0x8000_ffa1,
        ),
        (
            "scaled index and displacement use the original base",
            &[0x0f, 0xbf, 0x44, 0x88, 0x10][..],
            0x8000_3ff0,
            0xffff_80a1,
        ),
    ] {
        let mut image = byte_register_image(code);
        image.cpu.registers.eax = old_eax;
        image.cpu.registers.ecx = 4;
        image.map(0x80004, 0x8000, false);
        image.data(0x800f, &[0x5a, 0xa1, 0x80, 0x5a]);
        let mut expected = image.cpu;
        expected.registers.eax = eax;
        expected.eip += code.len() as u32;
        expected.instruction_count = 0;
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(expected.eip),
            }],
        );
    }
}

#[test]
fn failed_reads_leave_the_destination_flags_and_count_unchanged() {
    let step = TestModule::interpreter();
    for (name, opcode, address, frame, fault) in [
        (
            "missing byte source",
            0xbe,
            0x4020,
            None,
            Exit::PageFault {
                address: 0x4020,
                error: 0,
            },
        ),
        (
            "missing second word page",
            0xbf,
            0x4fff,
            Some(0x8000),
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
        (
            "word source rejects linear address wrap",
            0xb7,
            0xffff_ffff,
            Some(0x8000),
            Exit::PageFault {
                address: 0xffff_ffff,
                error: 0,
            },
        ),
        (
            "present byte frame outside guest RAM traps",
            0xb6,
            0x4000,
            Some(0x10000),
            Exit::Trap,
        ),
    ] {
        let code = [0x0f, opcode, 0x03];
        let mut image = byte_register_image(&code);
        image.cpu.registers.ebx = address;
        if let Some(frame) = frame {
            image.map(address >> 12, frame, false);
        }
        image.data(0x8fff, &[0x80]);
        both(
            step,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: fault,
            }],
        );
    }
}

const ALIASED_READ_THEN_FAULT: &[u8] = &[
    0x0f, 0xb7, 0x03, // MOVZX EAX, word [EBX]
    0x0f, 0xbe, 0xc0, // MOVSX EAX, AL
    0x66, 0x0f, 0xb6, 0xd4, // MOVZX DX, AH
    0x66, 0x89, 0x11, // MOV [ECX], DX, aliases the loaded word
    0x0f, 0xbf, 0xf0, // MOVSX ESI, AX
    0x0f, 0xbf, 0x3e, // MOVSX EDI, word [ESI], unmapped
];
const ALIASED_WORD_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0xff, 0x00])];

fn aliased_read_then_fault() -> (Image, Vec<Step<'static>>) {
    let mut image = byte_register_image(ALIASED_READ_THEN_FAULT);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0x6000;
    image.map(4, 0x8000, false);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0x5a, 0x80, 0xff, 0x5a]);
    let mut expected = image.cpu;
    let mut steps = Vec::new();
    for (index, (next, destination, value, ram)) in [
        (0x1003, Gpr32::Eax, 0x0000_ff80, &[][..]),
        (0x1006, Gpr32::Eax, 0xffff_ff80, &[][..]),
        (0x100a, Gpr32::Edx, 0xccbb_00ff, &[][..]),
        (0x100d, Gpr32::Edx, 0xccbb_00ff, ALIASED_WORD_WRITE),
        (0x1010, Gpr32::Esi, 0xffff_ff80, &[][..]),
    ]
    .into_iter()
    .enumerate()
    {
        expected.registers[destination] = value;
        expected.eip = next;
        expected.instruction_count = index as u32;
        steps.push(Step {
            cpu: expected,
            ram,
            exit: Exit::Dispatch(next),
        });
    }
    steps.push(Step {
        cpu: expected,
        ram: &[],
        exit: Exit::PageFault {
            address: 0xffff_ff80,
            error: 0,
        },
    });
    (image, steps)
}

#[test]
fn loaded_values_survive_alias_stores_and_publish_before_a_later_fault() {
    let (image, steps) = aliased_read_then_fault();
    both(
        TestModule::interpreter(),
        "extending moves retain completed progress",
        ALIASED_READ_THEN_FAULT,
        6,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn extending_moves_and_fault_publication_execute_in_optimizing_v8() {
    let (image, steps) = aliased_read_then_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        expected(&image, &steps),
        "interpreter",
    );
    let module = compile_block_from_bytes(image.cpu.eip, ALIASED_READ_THEN_FAULT, 6).unwrap();
    let block = TestModule::new(&module);
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: ALIASED_WORD_WRITE,
                exit: steps.last().unwrap().exit,
            }]
        ),
        "snapshot block",
    );
}

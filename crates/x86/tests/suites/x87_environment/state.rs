//! Complete legacy images preserve 80-bit payloads in ST order, then reset or restore.

use super::*;
use crate::support::x87::{complete_x87, real80};

fn instruction(format: Format, load: bool, address: u32) -> Vec<u8> {
    let mut code = format.instruction(load, address);
    let opcode = usize::from(code[0] == 0x66);
    code[opcode] = 0xdd;
    code
}

fn clear_pointers(cpu: &mut CpuState) {
    cpu.x87.opcode = 0;
    cpu.x87.instruction_offset = 0;
    cpu.x87.instruction_selector = 0;
    cpu.x87.data_offset = 0;
    cpu.x87.data_selector = 0;
}

fn initialize(cpu: &mut CpuState) {
    set_control(&mut cpu.x87.control, 0x037f);
    cpu.x87.status = status(0);
    cpu.x87.tag_word = 0xffff;
    clear_pointers(cpu);
}

fn payloads(cpu: &CpuState, order: [usize; 8]) -> Vec<u8> {
    order
        .into_iter()
        .flat_map(|index| {
            let register = cpu.x87.registers[index];
            real80((register.significand, register.sign_exponent))
        })
        .collect()
}

fn save_formats(frontend: Frontend) {
    for format in Format::ALL {
        let code = instruction(format, false, 0x4001);
        let mut image = format.image(&code);
        register_classes(&mut image.cpu);
        clear_pointers(&mut image.cpu);
        set_control(&mut image.cpu.x87.control, 0x035f);
        image.cpu.x87.status = status(0xa8a0); // TOP=5 and a pending precision exception.
        image.cpu.x87.tag_word = 0xea91;
        let mut bytes = format.bytes([
            0xffff_035f,
            0xffff_a8a0,
            0xffff_ea91,
            if format.real { 0xffff_0000 } else { 0 },
            0,
            if format.real { 0xffff_0000 } else { 0 },
            if format.real { 0 } else { 0xffff_0000 },
        ]);
        bytes.extend(payloads(&image.cpu, [5, 6, 7, 0, 1, 2, 3, 4]));
        let mut cpu = retire(image.cpu, code.len());
        initialize(&mut cpu);
        ImageSequences::new(frontend, format.profile()).check(
            &format!("FNSAVE {format:?} saves even empty and noncanonical payloads before reset"),
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8001, &bytes)],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn restore_formats(frontend: Frontend) {
    for format in Format::ALL {
        let restore = instruction(format, true, 0x4001);
        let code = [restore.as_slice(), &[0x9b]].concat();
        let mut image = format.image(&code);
        let mut source = image.cpu;
        register_classes(&mut source);
        let mut bytes = format.bytes([0x035f, 0x2820, 0xeaaa, 0, 0, 0, 0]);
        bytes.extend(payloads(&source, [5, 6, 7, 0, 1, 2, 3, 4]));
        image.data(0x8001, &bytes);
        let mut loaded = retire(image.cpu, restore.len());
        register_classes(&mut loaded);
        clear_pointers(&mut loaded);
        if !format.real && !format.wide {
            loaded.x87.opcode = image.cpu.x87.opcode;
        }
        set_control(&mut loaded.x87.control, 0x035f);
        loaded.x87.status = status(0xa8a0);
        loaded.x87.tag_word = 0xea91;
        ImageSequences::new(frontend, format.profile()).check(
            &format!(
                "FRSTOR {format:?} maps ST payloads to physical registers and defers exceptions"
            ),
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: loaded,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
}

fn saved_suppressed_push(frontend: Frontend) {
    let format = Format {
        real: false,
        wide: true,
    };
    let save = instruction(format, false, 0x4000);
    let code = [&[0xd9, 0xe8][..], &save].concat();
    let mut image = format.image(&code);
    image.cpu.x87.tag_word = 0;
    image.cpu.x87.status = status(0x0800);
    set_control(&mut image.cpu.x87.control, 0x037e);
    let mut faulted = complete_x87(image.cpu, 2, 0x01e8);
    faulted.x87.status = status(0x8ac1);
    let mut bytes = format.bytes([
        0xffff_037e,
        0xffff_8ac1,
        0xffff_0000,
        0x1000,
        0x01e8_001b,
        0x89ab_cdef,
        0xffff_0027,
    ]);
    bytes.extend(payloads(&image.cpu, [1, 2, 3, 4, 5, 6, 7, 0]));
    let mut saved = retire(faulted, save.len());
    initialize(&mut saved);
    ImageSequences::new(frontend, format.profile()).check(
        "FNSAVE uses actual TOP after a suppressed push without waiting",
        &code,
        &image,
        &[
            dispatch(faulted),
            Step {
                cpu: saved,
                ram: &[(0x8000, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
        ],
    );
}

fn saved_mmx_tags(frontend: Frontend) {
    let format = Format {
        real: false,
        wide: true,
    };
    let save = instruction(format, false, 0x4000);
    let code = [&[0x0f, 0xef, 0xc0][..], &save].concat(); // PXOR mm0, mm0; FNSAVE
    let mut image = format.image(&code);
    image.cpu.x87.status = status(0x1800);
    let mut mmx = retire(image.cpu, 3);
    mmx.x87.status.top = 0;
    mmx.x87.tag_word = 0;
    write_register_bits(&mut mmx, 0, (0, 0xffff));
    let mut bytes = format.bytes([
        0xffff_037f,
        0xffff_0000,
        0xffff_0002, // MMX leaves live tags valid; the saved R0 is special.
        0x1234_5678,
        0x0654_0017,
        0x89ab_cdef,
        0xffff_0027,
    ]);
    bytes.extend(payloads(&mmx, [0, 1, 2, 3, 4, 5, 6, 7]));
    let mut saved = retire(mmx, save.len());
    initialize(&mut saved);
    ImageSequences::new(frontend, format.profile()).check(
        "FNSAVE classifies MMX payloads in the saved image before initializing the FPU",
        &code,
        &image,
        &[
            dispatch(mmx),
            Step {
                cpu: saved,
                ram: &[(0x8000, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
        ],
    );
}

fn restore_then_pop(frontend: Frontend) {
    let format = Format {
        real: false,
        wide: true,
    };
    let restore = instruction(format, true, 0x4000);
    let code = [
        &[0xd9, 0xe8][..],
        restore.as_slice(),
        &[0xdb, 0x3d, 0x80, 0x40, 0, 0],
    ]
    .concat();
    let mut image = format.image(&code);
    image.cpu.x87.status = status(0x1800);
    image.cpu.x87.tag_word = 0xffff;
    let mut bytes = format.bytes([0x037f, 0x3800, 0x0030, 0, 0, 0, 0]);
    let zero = real80((0, 0x8000));
    bytes.extend(zero);
    for index in 0..7 {
        bytes.extend(real80((0x8000_0000_0000_0000 + index, 0x3fff)));
    }
    image.data(0x8000, &bytes);
    let mut pushed = complete_x87(image.cpu, 2, 0x01e8);
    pushed.x87.status = status(0x1000);
    pushed.x87.tag_word = 0xffcf;
    write_register_bits(&mut pushed, 2, (0x8000_0000_0000_0000, 0x3fff));
    let mut loaded = retire(pushed, restore.len());
    clear_pointers(&mut loaded);
    set_control(&mut loaded.x87.control, 0x037f);
    loaded.x87.status = status(0x3800);
    loaded.x87.tag_word = 0x4030;
    write_register_bits(&mut loaded, 7, (0, 0x8000));
    for index in 0..7 {
        write_register_bits(
            &mut loaded,
            index,
            (0x8000_0000_0000_0000 + index as u64, 0x3fff),
        );
    }
    let mut popped = complete_x87(loaded, 6, 0x033d);
    popped.x87.data_offset = 0x4080;
    popped.x87.data_selector = 0x23;
    popped.x87.status = status(0);
    popped.x87.tag_word = 0xc030;
    ImageSequences::new(frontend, format.profile()).check(
        "FRSTOR replaces earlier writes including empty payloads before a following pop",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(loaded),
            Step {
                cpu: popped,
                ram: &[(0x8080, &zero)],
                exit: Exit::Dispatch(popped.eip),
            },
        ],
    );
}

fn access_boundaries(frontend: Frontend) {
    for format in Format::ALL.into_iter().filter(|format| !format.real) {
        let length = if format.wide { 108 } else { 94 };
        for load in [false, true] {
            let code = instruction(format, load, 0x5001 - length);
            let mut image = format.image(&code);
            if !load {
                set_control(&mut image.cpu.x87.control, 0x037e);
                image.cpu.x87.status = status(0x8081);
            }
            ImageSequences::new(frontend, format.profile()).check(
                "the last register byte faults before save/reset or restore effects",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: if load { 0 } else { 2 },
                    },
                }],
            );
        }

        let code = instruction(format, true, 0x4fae);
        let mut image = format.image(&code);
        image.map(5, 0xc000, true);
        let mut bytes = format.bytes([0x037f, 0, 0, 0, 0, 0, 0]);
        for index in 0..8 {
            bytes.extend(real80((
                0x8011_2233_4455_6600 + index,
                0x3fff + index as u16,
            )));
        }
        image.data(0x8fae, &bytes[..82]);
        image.data(0xc000, &bytes[82..]);
        image.data(0x9000, &vec![0xa5; length as usize - 82]);
        let mut restored = retire(image.cpu, code.len());
        clear_pointers(&mut restored);
        if !format.wide {
            restored.x87.opcode = image.cpu.x87.opcode;
        }
        set_control(&mut restored.x87.control, 0x037f);
        restored.x87.status = status(0);
        restored.x87.tag_word = 0; // Eight occupied normal values.
        for index in 0..8 {
            write_register_bits(
                &mut restored,
                index,
                (0x8011_2233_4455_6600 + index as u64, 0x3fff + index as u16),
            );
        }
        ImageSequences::new(frontend, format.profile()).check(
            "FRSTOR reads a register split between noncontiguous backing pages",
            &code,
            &image,
            &[dispatch(restored)],
        );
    }
}

test_frontends!(state_save_formats, save_formats);
test_frontends!(state_restore_formats, restore_formats);
test_frontends!(state_save_suppressed_push, saved_suppressed_push);
test_frontends!(state_save_mmx_tag_classification, saved_mmx_tags);
test_frontends!(state_restore_then_pop, restore_then_pop);
test_frontends!(state_access_boundaries, access_boundaries);

#[test]
fn full_state_forms_consume_both_operand_sizes() {
    for format in Format::ALL.into_iter().filter(|format| !format.real) {
        for load in [false, true] {
            check_length(&instruction(format, load, 0x4000));
        }
    }
}

use super::{
    code_with_width, concrete_updates, expected, image as flag_image,
    machine::{both, check, Exit, Step},
    mask, recipe, register_result,
    step::ModuleFile,
    OPERATIONS, WIDTHS,
};

pub(super) fn check_memory(flags: &[&str], step: &ModuleFile) {
    for op in OPERATIONS {
        for bits in WIDTHS {
            let length = (bits / 8) as usize;
            for next_frame in [0x9000, 0xa000] {
                for destination_is_memory in [false, true] {
                    let opcode = op.opcode()
                        + u8::from(bits != 8)
                        + if destination_is_memory { 0 } else { 2 };
                    let code = code_with_width(bits, opcode, &[0x03]);
                    let mut image = flag_image(&code);
                    let (eax, memory) = if destination_is_memory {
                        (0x4433_0000 & !mask(bits), mask(bits))
                    } else {
                        (register_result(0x4433_0000, bits, mask(bits)), 0)
                    };
                    image.register(24, eax);
                    image.register(36, 0x4fff);
                    image.map(4, 0x8000, destination_is_memory);
                    image.map(5, next_frame, destination_is_memory);
                    let before = memory.to_le_bytes();
                    image.data(0x8ffe, &[0xa5, before[0]]);
                    image.data(next_frame, &before[1..length]);
                    image.data(next_frame + length as u32 - 1, &[0x5a]);
                    let result = expected(op, bits, mask(bits), 0, true);
                    let next = 0x1000 + code.len() as u32;
                    let mut changes = concrete_updates(&image, &result);
                    changes.extend_from_slice(&[(56, next), (144, 0)]);
                    if !destination_is_memory {
                        changes.push((24, register_result(eax, bits, result.result)));
                    }
                    let after = result.result.to_le_bytes();
                    let writes = [(0x8fff, &after[..1]), (next_frame, &after[1..length])];
                    both(step, flags, &format!("{op:?}/{bits} memory role {destination_is_memory}, frame {next_frame:#x}"),
                        &code, 1, &image, &[Step {
                            cpu: &changes,
                            ram: if destination_is_memory { &writes } else { &[] },
                            exit: Exit::Dispatch(next),
                        }]);
                }
            }
            let full = mask(bits).to_le_bytes();
            let mut immediates = vec![(
                0x80 + u8::from(bits != 8),
                full[..length].to_vec(),
                mask(bits),
            )];
            if bits != 8 {
                immediates.push((0x83, vec![0x80], 0xffff_ff80 & mask(bits)));
            }
            for (opcode, immediate, right) in immediates {
                // A negative displacement follows the group's ModRM before its immediate.
                let tail = [&[0x43 | (op.extension() << 3), 0x80], immediate.as_slice()].concat();
                let code = code_with_width(bits, opcode, &tail);
                let mut image = flag_image(&code);
                image.register(36, 0x40a0);
                image.map(4, 0x8000, true);
                image.data(0x801f, &[0xa5; 6]);
                image.data(0x8020, &1u32.to_le_bytes()[..length]);
                let result = expected(op, bits, 1, right, true);
                let next = 0x1000 + code.len() as u32;
                let mut changes = concrete_updates(&image, &result);
                changes.extend_from_slice(&[(56, next), (144, 0)]);
                let after = result.result.to_le_bytes();
                both(
                    step,
                    flags,
                    "carry group uses memory displacement and immediate widths",
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: &changes,
                        ram: &[(0x8020, &after[..length])],
                        exit: Exit::Dispatch(next),
                    }],
                );
            }
        }
        for (opcode, eax, memory, left, right, changes_eax) in [
            (op.opcode(), 0x4020, 0xdf, 0xdf, 0x20, false),
            (op.opcode() + 3, 0x4020, 5, 0x4020, 5, true),
        ] {
            let code = [opcode, 0x00];
            let mut image = flag_image(&code);
            image.register(24, eax);
            image.map(4, 0x8000, !changes_eax);
            image.data(0x801f, &[0xa5; 6]);
            let bits = if changes_eax { 32 } else { 8 };
            image.data(0x8020, &u32::to_le_bytes(memory)[..(bits / 8) as usize]);
            let result = expected(op, bits, left, right, true);
            let mut changes = concrete_updates(&image, &result);
            changes.extend_from_slice(&[(56, 0x1002), (144, 0)]);
            if changes_eax {
                changes.push((24, result.result));
            }
            let after = [result.result as u8];
            let writes = [(0x8020, after.as_slice())];
            both(
                step,
                flags,
                "carry operand uses the original address register",
                &code,
                1,
                &image,
                &[Step {
                    cpu: &changes,
                    ram: if changes_eax { &[] } else { &writes },
                    exit: Exit::Dispatch(0x1002),
                }],
            );
        }
    }
}

pub(super) fn check_faults(flags: &[&str], step: &ModuleFile) {
    for op in OPERATIONS {
        for bits in WIDTHS {
            for (memory_destination, first_writable, tail_writable, fault) in [
                (true, None, None, 0x0004_0002_0000_4fff),
                (false, None, None, 0x0004_0000_0000_4fff),
                (true, Some(false), None, 0x0004_0003_0000_4fff),
                (true, Some(true), None, 0x0004_0002_0000_5000),
                (true, Some(true), Some(false), 0x0004_0003_0000_5000),
                (false, Some(false), None, 0x0004_0000_0000_5000),
            ] {
                if bits == 8 && fault & 0xffff_ffff == 0x5000 {
                    continue;
                }
                let code = code_with_width(
                    bits,
                    op.opcode() + u8::from(bits != 8) + if memory_destination { 0 } else { 2 },
                    &[0x03],
                );
                let mut image = flag_image(&code);
                // Resolving this invalid record would trap. An operand fault must win.
                image.cpu[0] = 0xff;
                image.register(36, 0x4fff);
                if let Some(writable) = first_writable {
                    image.map(4, 0x8000, writable);
                }
                if let Some(writable) = tail_writable {
                    image.map(5, 0xa000, writable);
                }
                image.data(0x8ffe, &[0xa5, 0xff]);
                image.data(0xa000, &[0xff, 0xff, 0xff, 0x5a]);
                both(
                    step,
                    flags,
                    &format!("{op:?}/{bits} proves the full operand before reading CF"),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: &[],
                        ram: &[],
                        exit: Exit::Fault(fault),
                    }],
                );
            }
        }
        let code = [0x01, 0xd1, op.opcode() + 1, 0x03];
        let mut image = flag_image(&code);
        image.register(24, 0);
        image.register(28, 0xffff_ffff);
        image.register(32, 1);
        image.register(36, 0x4fff);
        image.map(4, 0x8000, true);
        image.data(0x8ffe, &[0xa5, 0xff]);
        let mut first = recipe(10, 0xffff_ffff, 1).to_vec();
        first.extend_from_slice(&[(28, 0), (56, 0x1002), (144, 0)]);
        both(
            step,
            flags,
            "failed carry RMW publishes prior completed arithmetic",
            &code,
            2,
            &image,
            &[
                Step {
                    cpu: &first,
                    ram: &[],
                    exit: Exit::Dispatch(0x1002),
                },
                Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(0x0004_0002_0000_5000),
                },
            ],
        );

        let code = [op.opcode() + 1, 0xd1, op.opcode() + 1, 0x03];
        let mut image = flag_image(&code);
        image.register(28, 0xffff_ffff);
        image.register(32, 1);
        image.register(36, 0x4fff);
        image.map(4, 0x8000, true);
        image.data(0x8ffe, &[0xa5, 0xff]);
        let result = expected(op, 32, 0xffff_ffff, 1, true);
        let mut first = concrete_updates(&image, &result);
        first.extend_from_slice(&[(28, result.result), (56, 0x1002), (144, 0)]);
        both(
            step,
            flags,
            "operand fault publishes the prior completed carry source",
            &code,
            2,
            &image,
            &[
                Step {
                    cpu: &first,
                    ram: &[],
                    exit: Exit::Dispatch(0x1002),
                },
                Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(0x0004_0002_0000_5000),
                },
            ],
        );

        let group = op.extension() << 3;
        for code in [
            vec![op.opcode()],
            vec![op.opcode() + 1, 0x04],
            vec![op.opcode() + 3, 0x05, 0, 0x40, 0],
            vec![0x80, group | 0x05, 0, 0x40, 0, 0],
            vec![0x81, group | 0x04, 0x25, 0, 0x40, 0, 0, 0xff, 0xff],
            vec![0x66, 0x81, group | 0x05, 0, 0x40, 0, 0, 0xff],
            vec![0x83, group | 0x05, 0, 0x40, 0, 0],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = flag_image(&[]);
            image.cpu[0] = 0xff;
            image.register(56, start);
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                step,
                flags,
                "carry fetch fault precedes operand access and CF resolution",
                &image,
                &[Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(0x0004_0010_0000_2000),
                }],
            );
        }
        for suffix in [vec![op.opcode()], vec![0x81, group | 0xc0, 1]] {
            let code = [vec![0x66; 15 - suffix.len()], suffix].concat();
            let mut image = flag_image(&[]);
            image.cpu[0] = 0xff;
            image.register(56, 0x1ff1);
            image.data(0x3ff1, &code);
            check(
                step,
                flags,
                "carry field beyond byte fifteen raises length before fetch",
                &image,
                &[Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(0x0002_0000_0000_0000),
                }],
            );
        }
    }
}

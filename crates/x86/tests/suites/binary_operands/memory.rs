use super::{
    image,
    machine::{both, Exit, Step},
    recipe,
    step::TestModule,
};

#[test]
fn memory_operands_preserve_aliases_and_partial_registers() {
    let step = TestModule::interpreter();
    for (name, code, eax, memory, result, kind, left, right) in [
        (
            "ADD register reads a readonly source",
            &[0x03, 0x03][..],
            0x10,
            &[0xf0, 0xff, 0xff, 0xff][..],
            0,
            10,
            0x10,
            0xffff_fff0,
        ),
        (
            "SUB register reads memory as its right operand",
            &[0x2b, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            0x8000_0001,
            9,
            1,
            0x8000_0000,
        ),
        (
            "CMP memory needs no write permission",
            &[0x39, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            1,
            9,
            0x8000_0000,
            1,
        ),
        (
            "CMP register reads memory in operand order",
            &[0x3b, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            1,
            9,
            1,
            0x8000_0000,
        ),
        (
            "ADD reads old address register",
            &[0x03, 0x00][..],
            0x4000,
            &[5, 0, 0, 0][..],
            0x4005,
            10,
            0x4000,
            5,
        ),
    ] {
        let mut image = image(code);
        image.register(24, eax);
        image.register(36, 0x4000);
        image.map(4, 0x8000, false);
        image.data(0x8000, memory);
        let next = 0x1000 + code.len() as u32;
        let mut changes = recipe(kind, left, right).to_vec();
        changes.extend_from_slice(&[(24, result), (56, next), (144, 0)]);
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram: &[],
                exit: Exit::Dispatch(next),
            }],
        );
    }
    for (name, next_frame) in [("contiguous RMW", 0x9000), ("scattered RMW", 0xa000)] {
        let code = [0x01, 0x03];
        let mut image = image(&code);
        image.register(24, 1);
        image.register(36, 0x4ffe);
        image.map(4, 0x8000, true);
        image.map(5, next_frame, true);
        image.data(0x8ffd, &[0xa5, 0xff, 0xff]);
        image.data(next_frame, &[0xff, 0x7f, 0x5a]);
        both(
            step,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[
                    (0, 0xa5a5_a50a),
                    (4, 0x7fff_ffff),
                    (8, 1),
                    (56, 0x1002),
                    (144, 0),
                ],
                ram: &[(0x8ffe, &[0, 0]), (next_frame, &[0, 0x80])],
                exit: Exit::Dispatch(0x1002),
            }],
        );
    }
    for (name, code, eax, before, after, kind, left, right) in [
        (
            "byte RMW uses original address AL",
            &[0x00, 0x00][..],
            0x4020,
            &[0xe0][..],
            &[0][..],
            2,
            0xe0,
            0x20,
        ),
        (
            "word RMW wraps only the selected width",
            &[0x66, 0x01, 0x43, 0x80][..],
            1,
            &[0xff, 0xff][..],
            &[0, 0][..],
            6,
            0xffff,
            1,
        ),
        (
            "byte SUB RMW records borrow and truncates its store",
            &[0x28, 0x03][..],
            1,
            &[0][..],
            &[0xff][..],
            1,
            0,
            1,
        ),
        (
            "word SUB RMW preserves its operand width",
            &[0x66, 0x29, 0x43, 0x80][..],
            1,
            &[0, 0x80][..],
            &[0xff, 0x7f][..],
            5,
            0x8000,
            1,
        ),
        (
            "SUB memory reads the same operands in reverse order",
            &[0x29, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            &[0xff, 0xff, 0xff, 0x7f][..],
            9,
            0x8000_0000,
            1,
        ),
        (
            "group byte immediate RMW",
            &[0x80, 0x03, 0x80][..],
            1,
            &[0x80][..],
            &[0][..],
            2,
            0x80,
            0x80,
        ),
    ] {
        let mut image = image(code);
        image.register(24, eax);
        image.register(36, if code[0] == 0x66 { 0x40a0 } else { 0x4020 });
        image.map(4, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xcc, 0xcc, 0x5a]);
        image.data(0x8020, before);
        let next = 0x1000 + code.len() as u32;
        let mut changes = recipe(kind, left, right).to_vec();
        changes.extend_from_slice(&[(56, next), (144, 0)]);
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram: &[(0x8020, after)],
                exit: Exit::Dispatch(next),
            }],
        );
    }
}

#[test]
fn logical_memory_operands_publish_flags() {
    let step = TestModule::interpreter();
    for (name, code, eax, before, after, kind, result) in [
        (
            "byte TEST reads AH and readonly memory",
            &[0x84, 0x23][..],
            0x4433_8001,
            &[0xf0][..],
            None,
            3,
            0x80,
        ),
        (
            "word immediate TEST reads a readonly page span",
            &[0x66, 0xf7, 0x03, 0x00, 0xff][..],
            0x4433_8001,
            &[0xff, 0x80][..],
            None,
            7,
            0x8000,
        ),
        (
            "dword TEST preserves memory and its source register",
            &[0x85, 0x03][..],
            0xffff_00ff,
            &[1, 0, 0, 0x80][..],
            None,
            11,
            0x8000_0001,
        ),
        (
            "byte AND updates memory using old AH",
            &[0x20, 0x23][..],
            0x4433_8001,
            &[0xf3][..],
            Some(&[0x80][..]),
            3,
            0x80,
        ),
        (
            "word OR updates exactly two bytes",
            &[0x66, 0x09, 0x03][..],
            0x4433_0001,
            &[0, 0x80][..],
            Some(&[1, 0x80][..]),
            7,
            0x8001,
        ),
        (
            "dword XOR updates the whole proven page span",
            &[0x31, 0x03][..],
            0x8000_0000,
            &[0xff, 0xff, 0xff, 0xff][..],
            Some(&[0xff, 0xff, 0xff, 0x7f][..]),
            11,
            0x7fff_ffff,
        ),
    ] {
        for next_frame in [0x9000, 0xa000] {
            let mut image = image(code);
            image.register(24, eax);
            image.register(36, 0x4fff);
            image.map(4, 0x8000, after.is_some());
            image.map(5, next_frame, after.is_some());
            image.data(0x8ffe, &[0xa5, before[0]]);
            image.data(next_frame, &before[1..]);
            image.data(next_frame + before.len() as u32 - 1, &[0x5a]);
            let next = 0x1000 + code.len() as u32;
            let changes = [(0, 0xa5a5_a500 | kind), (4, result), (56, next), (144, 0)];
            let writes = after.map(|bytes| [(0x8fff, &bytes[..1]), (next_frame, &bytes[1..])]);
            both(
                step,
                name,
                code,
                1,
                &image,
                &[Step {
                    cpu: &changes,
                    ram: writes
                        .as_ref()
                        .map(|writes| writes.as_slice())
                        .unwrap_or(&[]),
                    exit: Exit::Dispatch(next),
                }],
            );
        }
    }
}

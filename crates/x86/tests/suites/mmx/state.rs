use super::*;
use crate::support::x87::{complete_x87, register_bits, set_control, status};

// EMMS; FLD m64; MOVD eax,mm7; FDECSTP; FADD ST0,ST0; FSTP m64.
const READ_THROUGH_MMX: [u8; 21] = [
    0x0f, 0x77, 0xdd, 0x05, 0, 0x40, 0, 0, 0x0f, 0x7e, 0xf8, 0xd9, 0xf6, 0xd8, 0xc0, 0xdd, 0x1d, 8,
    0x40, 0, 0,
];

fn transitions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // FLD ST0; FSTP ST0; MOVQ mm1,mm7; EMMS; FILD dword [0x4000].
    let code = [
        0xd9, 0xc0, 0xdd, 0xd8, 0x0f, 0x6f, 0xcf, 0x0f, 0x77, 0xdb, 0x05, 0, 0x40, 0, 0,
    ];
    let mut image = stack_image(&code, 0, 0xfffc);
    image.map(4, 0x8000, false);
    image.data(0x8000, &1_u32.to_le_bytes());
    let mut pushed = complete_x87(image.cpu, 2, 0x01c0);
    pushed.x87.status.c1 = 0;
    pushed.x87.status.top = 7;
    pushed.x87.tag_word = 0x3ffc;
    write_register_bits(&mut pushed, 7, register_bits(&image.cpu, 0));
    let mut popped = complete_x87(pushed, 2, 0x05d8);
    popped.x87.status.top = 0;
    popped.x87.tag_word = 0xfffc;
    let mut moved = complete(popped, 3);
    write_register_bits(&mut moved, 1, (pushed.x87.registers[7].significand, 0xffff));
    let mut emptied = complete(moved, 2);
    emptied.x87.tag_word = 0xffff;
    let mut loaded = complete_x87(emptied, 6, 0x0305);
    loaded.x87.status.top = 7;
    loaded.x87.tag_word = 0x3fff;
    loaded.x87.data_offset = 0x4000;
    loaded.x87.data_selector = 0x23;
    write_register_bits(&mut loaded, 7, (0x8000_0000_0000_0000, 0x3fff));
    checks.check(
        "x87 cached values become physical MMX values and EMMS permits a push",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(popped),
            dispatch(moved),
            dispatch(emptied),
            dispatch(loaded),
        ],
    );

    // MOVQ mm0,mm1; FSTP tbyte [0x4000] observes the MMX payload and exponent.
    let code = [0x0f, 0x6f, 0xc1, 0xdb, 0x3d, 0, 0x40, 0, 0];
    let mut image = super::image(&code);
    image.map(4, 0x8000, true);
    let bits = (image.cpu.x87.registers[1].significand, 0xffff);
    let mut moved = complete(image.cpu, 3);
    write_register_bits(&mut moved, 0, bits);
    let mut stored = complete_x87(moved, 6, 0x033d);
    stored.x87.status.c1 = 0;
    stored.x87.status.top = 1;
    stored.x87.tag_word = 3;
    stored.x87.data_offset = 0x4000;
    stored.x87.data_selector = 0x23;
    let bytes = crate::support::x87::real80(bits);
    checks.check(
        "x87 raw store sees cached MMX destination",
        &code,
        &image,
        &[
            dispatch(moved),
            Step {
                cpu: stored,
                ram: &[(0x8000, &bytes)],
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );

    let code = [0x0f, 0x77];
    let image = super::image(&code);
    let mut emptied = complete(image.cpu, 2);
    emptied.x87.tag_word = 0xffff;
    checks.check(
        "standalone EMMS resets TOP and tags while preserving every payload",
        &code,
        &image,
        &[dispatch(emptied)],
    );

    // The MMX read preserves the floating-point value in physical register 7.
    let code = READ_THROUGH_MMX;
    let mut image = super::image(&code);
    set_control(&mut image.cpu.x87.control, 0x027f);
    image.map(4, 0x8000, true);
    image.data(0x8000, &1.5_f64.to_le_bytes());
    let mut emptied = complete(image.cpu, 2);
    emptied.x87.tag_word = 0xffff;
    let mut loaded = complete_x87(emptied, 6, 0x0505);
    loaded.x87.status.top = 7;
    loaded.x87.status.c1 = 0;
    loaded.x87.tag_word = 0x3fff;
    loaded.x87.data_offset = 0x4000;
    loaded.x87.data_selector = 0x23;
    write_register_bits(&mut loaded, 7, (0xc000_0000_0000_0000, 0x3fff));
    let mut moved = complete(loaded, 3);
    moved.registers.eax = 0;
    let mut rotated = complete_x87(moved, 2, 0x01f6);
    rotated.x87.status.top = 7;
    let mut added = complete_x87(rotated, 2, 0x00c0);
    write_register_bits(&mut added, 7, (0xc000_0000_0000_0000, 0x4000));
    let mut stored = complete_x87(added, 6, 0x051d);
    stored.x87.status.top = 0;
    stored.x87.tag_word = 0xc000;
    stored.x87.data_offset = 0x4008;
    checks.check(
        "MMX read retains the x87 value across TOP reset and rotation",
        &code,
        &image,
        &[
            dispatch(emptied),
            dispatch(loaded),
            dispatch(moved),
            dispatch(rotated),
            dispatch(added),
            Step {
                cpu: stored,
                ram: &[(0x8008, &3_f64.to_le_bytes())],
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );
}

fn suppressed_write(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // A suppressed load must not become visible when FNCLEX allows MMX to continue.
    let code = [
        0xd9, 0xc0, 0xdd, 0xd8, 0xdd, 0x05, 0, 0x40, 0, 0, 0xdb, 0xe2, 0x0f, 0x6f, 0xcf,
    ];
    let mut image = stack_image(&code, 0, 0xfffc);
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.cpu.x87.status = status(0);
    image.map(4, 0x8000, false);
    image.data(0x8000, &0x7ff0_0000_0000_0001_u64.to_le_bytes());
    let mut pushed = complete_x87(image.cpu, 2, 0x01c0);
    pushed.x87.status.top = 7;
    pushed.x87.tag_word = 0x3ffc;
    write_register_bits(&mut pushed, 7, register_bits(&image.cpu, 0));
    let mut popped = complete_x87(pushed, 2, 0x05d8);
    popped.x87.status.top = 0;
    popped.x87.tag_word = 0xfffc;
    let mut suppressed = complete_x87(popped, 6, 0x0505);
    suppressed.x87.status = status(0x8081);
    suppressed.x87.data_offset = 0x4000;
    suppressed.x87.data_selector = 0x23;
    let mut cleared = suppressed;
    cleared.eip += 2;
    cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
    cleared.x87.status = status(0);
    let mut moved = complete(cleared, 3);
    write_register_bits(&mut moved, 1, (pushed.x87.registers[7].significand, 0xffff));
    checks.check(
        "MMX after clearing a suppressed x87 write reads retained payload",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(popped),
            dispatch(suppressed),
            dispatch(cleared),
            dispatch(moved),
        ],
    );
}

test_frontends!(mmx_x87_transitions, transitions);
test_frontends!(mmx_suppressed_x87_write, suppressed_write);

#[test]
fn mmx_read_preserves_native_x87_arithmetic() {
    use wasm86_x86::Compiler;
    use wasmparser::{Operator, Parser, Payload};
    let mut observed = CpuState::default();
    set_control(&mut observed.x87.control, 0x027f);
    let module = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&observed)
        .compile_block(0x1000, &READ_THROUGH_MMX, 6)
        .unwrap();
    assert!(Parser::new(0).parse_all(&module.bytes).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if
            body.get_operators_reader().unwrap().into_iter().any(|op| matches!(op.unwrap(), Operator::F64Add)))
    }));
}

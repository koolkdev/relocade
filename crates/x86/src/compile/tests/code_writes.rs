use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{Event, TestModule},
    },
    BlockSnapshot, Compiler, CpuState, ExecutionProfile, Segments,
};

fn interpreter() -> &'static TestModule {
    TestModule::interpreter_run_with_profile(ExecutionProfile::Real16)
}

fn self_modification() {
    // The store changes the following MOV's immediate through a writable alias.
    let code = [
        0xb8, 0x78, 0x56, 0x89, 0x07, 0xb8, 0x11, 0x11, 0xba, 0x34, 0x12, 0xeb, 0,
    ];
    let blocks = Compiler::new(ExecutionProfile::Real16)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &code,
                instruction_limit: 5,
            },
            BlockSnapshot {
                start_eip: 0x1005,
                bytes: &code[5..],
                instruction_limit: 3,
            },
        ])
        .unwrap();
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: 0x1000,
        instruction_count: 10,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    image.cpu.registers.ebx = 0x2006;
    image.map(1, 0x1000, true);
    image.map(2, 0x1000, true);
    image.data(0x1000, &code);
    let mut input = image.input();
    input.watched_backing_pages = vec![1];
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    let mut prefix = image.cpu;
    prefix.registers.eax = 0x5678;
    prefix.eip = 0x1003;
    prefix.instruction_count = 11;
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: prefix,
                ram: &[],
                exit: Exit::Interpret
            }]
        )
    );

    let module = module.with_interpreter(interpreter());
    let mut after = prefix;
    after.registers.edx = 0x1234;
    after.eip = 0x100d;
    after.instruction_count = 15;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu: after,
            ram: &[(0x1006, &[0x78, 0x56])],
            exit: Exit::Dispatch(0x100d),
        }],
    );
    wanted.events.splice(
        0..0,
        [
            Event::CodeWrite {
                backing: 0x1006,
                value: vec![0x78],
            },
            Event::CodeWrite {
                backing: 0x1007,
                value: vec![0x56],
            },
        ],
    );
    assert_eq!(module.observe(&input, 1), wanted);
}

fn partial_effects() {
    for code in [
        &[0x60][..],
        &[0x61],
        &[0xc8, 0, 0, 1],
        &[0xcc],
        &[0x6c],
        &[0xf3, 0x6e],
        &[0xf3, 0xab],
    ] {
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let module = TestModule::new(&compiler.compile_block(0x1000, code, 1).unwrap());
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: 0x1000,
            instruction_count: 10,
            segments: Segments::real_mode(),
            ..CpuState::default()
        };
        image.cpu.registers.ecx = 3;
        image.cpu.registers.edi = 0x3000;
        // PUSHA first writes two ordinary words before reaching watched RAM.
        // POPA first reads two ordinary words before reaching the watched page.
        image.cpu.registers.eax = 0x1234;
        image.cpu.registers.esp = if code == [0x61] { 0x3ffc } else { 0x4004 };
        image.cpu.registers.ebp = 0x3002;
        image.map(1, 0x1000, false);
        image.map(3, 0x3000, true);
        image.map(4, 0x4000, true);
        image.data(0x1000, code);
        let mut input = image.input();
        input.watched_backing_pages = vec![if code == [0x61] { 4 } else { 3 }];
        assert_eq!(
            module.observe(&input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Interpret
                }]
            ),
            "{code:x?}"
        );
    }
}

fn direct_repetition() {
    let code = [0xf3, 0xab]; // REP STOSW
    let module = TestModule::new(
        &Compiler::new(ExecutionProfile::Real16)
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    );
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: 0x1000,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    image.cpu.registers.eax = 0x1234;
    image.cpu.registers.ecx = 3;
    image.cpu.registers.edi = 0x3000;
    image.map(3, 0x5000, true);
    let mut after = image.cpu;
    after.eip = 0x1002;
    after.instruction_count = 1;
    after.registers.ecx = 0;
    after.registers.edi = 0x3006;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[(0x5000, &[0x34, 0x12, 0x34, 0x12, 0x34, 0x12])],
                exit: Exit::Dispatch(0x1002)
            }]
        )
    );
}

#[test]
fn watched_store_abandons_the_captured_suffix() {
    self_modification();
}
#[test]
fn partial_effects_restart_before_the_instruction() {
    partial_effects();
    direct_repetition();
}

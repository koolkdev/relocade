use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set, Undefined},
        Flags,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::Gpr32::{Eax, Ebx};

use crate::support::{
    arithmetic::image,
    machine::{check, Exit, Step},
    step::TestModule,
};
use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

#[rustfmt::skip]
fn result_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("byte AND retains upper EAX and clears stale carry and overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_22f3).initial_register(Ebx, 0x0f)
            .step(Checkpoint::new(&[0x20, 0xd8],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x4433_2203))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("word AND uses the word sign and only the low byte for parity", Flags::all(true))
            .initial_register(Eax, 0x4433_80ff).initial_register(Ebx, 0xdead_ff00)
            .step(Checkpoint::new(&[0x66, 0x21, 0xd8],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_8000))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("reverse dword AND records a negative odd-parity result", Flags::all(true))
            .initial_register(Eax, 0x8000_0001).initial_register(Ebx, 0xffff_ffff)
            .step(Checkpoint::new(&[0x23, 0xc3],
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x8000_0001))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("group AND sign extends its byte mask to a dword", Flags::all(true))
            .initial_register(Eax, 0x89ab_cdef).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x83, 0xe0, 0xff],
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x89ab_cdef))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("byte OR reads old AL before replacing AH", Flags::all(true))
            .initial_register(Eax, 0x4433_8001).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x0a, 0xe0],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_8101))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("group OR sign extends its byte mask to a word", Flags::all(true))
            .initial_register(Eax, 0x4433_0001).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x66, 0x83, 0xc8, 0x80],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_ff81))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("dword accumulator OR ignores upper bits for parity", Flags::all(true))
            .initial_register(Eax, 0x100).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x0d, 1, 0, 0, 0],
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x101))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("self XOR clears only AH and produces equal flags", Flags::all(true))
            .initial_register(Eax, 0x4433_ff80).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x30, 0xe4],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x4433_0080))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("reverse word XOR preserves the upper parent register", Flags::all(true))
            .initial_register(Eax, 0x4433_ffff).initial_register(Ebx, 0xdead_8000)
            .step(Checkpoint::new(&[0x66, 0x33, 0xc3],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x4433_7fff))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("group XOR sign extends its byte mask to a dword", Flags::all(true))
            .initial_register(Eax, 0x8000_0000).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x83, 0xf0, 0xff],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x7fff_ffff))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("TEST reads AH and AL without replacing either alias", Flags::all(true))
            .initial_register(Eax, 0x4433_807f).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x84, 0xc4],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x4433_807f))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("word accumulator TEST leaves EAX unchanged", Flags::all(true))
            .initial_register(Eax, 0x4433_8001).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x66, 0xa9, 0x00, 0xff],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_8001))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("dword register TEST keeps both inputs", Flags::all(true))
            .initial_register(Eax, 0x8000_0001).initial_register(Ebx, 0xffff_ffff)
            .step(Checkpoint::new(&[0x85, 0xd8],
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x8000_0001))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("byte group TEST ignores the operand-size prefix", Flags::all(true))
            .initial_register(Eax, 0x4433_2280).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x66, 0xf6, 0xc0, 0x80],
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_2280))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("dword group TEST publishes its result without a write", Flags::all(true))
            .initial_register(Eax, 0xffff_fff0).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0xf7, 0xc0, 0x0f, 0, 0, 0],
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Set, sf: Clear, of: Clear }).register(Eax, 0xffff_fff0))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
    ]
}

test_sequences!(results_and_conditions, result_conditions());

#[test]
fn discarded_arithmetic_operands_remain_unpublished_after_logic() {
    let step = TestModule::interpreter();
    let code = [
        0x05, 1, 0, 0, 0, // ADD EAX,1
        0x31, 0xc0, // XOR EAX,EAX
        0x0f, 0x94, 0xc4, // SETE AH
        0xb0, 0x7f, // MOV AL,7f
        0x0f, 0x92, 0xc0, // SETB AL
    ];
    let mut image = image(&code);
    image.cpu.registers.eax = 0xffff_ffff;
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 10;
    expected_cpu.flags.left = 0xffff_ffff;
    expected_cpu.flags.right = 1;
    expected_cpu.registers.eax = 0;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.flags.kind = 11;
    expected_cpu.flags.left = 0;
    expected_cpu.eip = 0x1007;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    expected_cpu.registers.eax = 0x100;
    expected_cpu.eip = 0x100a;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100a),
    });

    expected_cpu.registers.eax = 0x17f;
    expected_cpu.eip = 0x100c;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100c),
    });

    expected_cpu.registers.eax = 0x100;
    expected_cpu.eip = 0x100f;
    expected_cpu.instruction_count = 4;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100f),
    });

    check(
        step,
        "published arithmetic B remains unused after logic",
        &image,
        &steps,
    );

    let snapshot = compile_block_from_bytes(0x1000, &code, 5).unwrap();
    Validator::new().validate_all(&snapshot.bytes).unwrap();
    // One snapshot never publishes the replaced ADD record: its unused B stays
    // at the original backing value. Both paths expose the same logical flags.
    expected_cpu.flags.right = image.cpu.flags.right;

    check(
        &TestModule::new(&snapshot),
        "logic discards an unpublished arithmetic B",
        &image,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100f),
        }],
    );
}

#[rustfmt::skip]
fn replacement_sequence() -> Vec<SequenceCase> {
    vec![SequenceCase::new("arithmetic replaces a logical result", Flags::all(true))
        .initial_register(Eax, 0x4433_8001)
        .step(Checkpoint::new(&[0x66, 0x25, 0xff, 0],
            Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001))
        .step(Checkpoint::new(&[0x2d, 1, 0, 0x33, 0x44],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc4]).register(Eax, 0x100))]
}

test_sequences!(logic_then_arithmetic, replacement_sequence());

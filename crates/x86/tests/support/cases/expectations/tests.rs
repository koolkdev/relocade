use std::panic::{catch_unwind, AssertUnwindSafe};

use wasm86_x86::Gpr32::{Eax, Ecx};

use super::{check_flags, check_state, validate};
use crate::support::{
    cases::{FlagExpectation, Flags, InstructionCase, MemoryExpectation, RegisterExpectation},
    guest::{Execution, Exit, Machine, Permissions, State},
};

struct Fixture {
    case: InstructionCase,
    initial: State,
    actual: State,
}

impl Fixture {
    fn new() -> Self {
        let mut machine = Machine::new(&[0x90]);
        machine.memory(0x4000, &[0xa5, 0, 0x5a], Permissions::ReadWrite);
        let initial = machine.state();
        let mut actual = initial.clone();
        actual.cpu.eip = 0x1001;
        actual.cpu.instruction_count = 0;
        Self {
            case: InstructionCase::new(
                "comparison contract",
                &[0x90],
                Flags {
                    cf: true,
                    pf: false,
                    af: false,
                    zf: true,
                    sf: true,
                    of: false,
                },
                Flags {
                    cf: FlagExpectation::Preserved,
                    pf: FlagExpectation::Undefined,
                    af: FlagExpectation::Set,
                    zf: FlagExpectation::Clear,
                    sf: FlagExpectation::Undefined,
                    of: FlagExpectation::Preserved,
                },
            )
            .memory(0x4000, &[0xa5, 0, 0x5a], Permissions::ReadWrite),
            initial,
            actual,
        }
    }

    fn check(&self) {
        let execution = Execution {
            state: self.actual.clone(),
            exit: Exit::Dispatch(0x1001),
            dispatches: vec![(0x1001, self.actual.clone())],
            machine_unchanged: true,
        };
        check_state(&self.case, &self.initial, &execution, &self.case.name);
    }
}

fn rejects(check: impl FnOnce(), expected_message: &str) {
    let error = catch_unwind(AssertUnwindSafe(check)).expect_err("the incorrect state must fail");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(
        message.contains(expected_message),
        "unexpected diagnostic: {message}"
    );
    assert!(
        message.contains("comparison contract"),
        "missing case name: {message}"
    );
}

#[test]
fn a_register_mask_ignores_only_undefined_bits() {
    let mut fixture = Fixture::new();
    fixture.case.expected.registers = vec![(
        Eax,
        RegisterExpectation::DefinedBits {
            value: 0x1111_0000,
            mask: 0xffff_0000,
        },
    )];
    fixture.actual.cpu.registers.eax = 0x1111_beef;
    fixture.check();
    fixture.actual.cpu.registers.eax = 0x2222_beef;
    rejects(|| fixture.check(), "Eax");
    fixture.actual.cpu.registers.eax = 0x1111_beef;
    fixture.actual.cpu.registers.ecx = 0;
    rejects(|| fixture.check(), "Ecx");
}

#[test]
fn exact_memory_and_undefined_spans_still_check_neighboring_bytes() {
    let mut fixture = Fixture::new();
    fixture.case.expected.memory = vec![MemoryExpectation::Exact {
        address: 0x4001,
        bytes: vec![7],
    }];
    rejects(|| fixture.check(), "guest memory");
    fixture.actual.memory.write(0x4001, &[7]);
    fixture.check();
    fixture.case.expected.memory = vec![MemoryExpectation::Undefined {
        address: 0x4001,
        length: 1,
    }];
    fixture.actual.memory.write(0x4001, &[0xff]);
    fixture.check();
    fixture.actual.memory.write(0x4000, &[0]);
    rejects(|| fixture.check(), "guest memory");
}

#[test]
fn preserved_flags_are_checked_while_undefined_flags_are_unconstrained() {
    let fixture = Fixture::new();
    let mut actual = Flags {
        cf: true,
        pf: true,
        af: true,
        zf: false,
        sf: false,
        of: false,
    };
    check_flags(&fixture.case, actual, &fixture.case.name);
    actual.cf = false;
    rejects(
        || check_flags(&fixture.case, actual, &fixture.case.name),
        "CF (Preserved)",
    );
    actual.cf = true;
    actual.af = false;
    rejects(
        || check_flags(&fixture.case, actual, &fixture.case.name),
        "AF (Set)",
    );
    actual.af = true;
    actual.zf = true;
    rejects(
        || check_flags(&fixture.case, actual, &fixture.case.name),
        "ZF (Clear)",
    );
}

#[test]
fn duplicate_registers_and_overlapping_memory_expectations_are_rejected() {
    let mut fixture = Fixture::new();
    fixture.case.initial.registers = vec![(Ecx, 1), (Ecx, 2)];
    rejects(|| validate(&fixture.case), "duplicate register Ecx");
    fixture.case.initial.registers = vec![];
    fixture.case.expected.registers = vec![
        (Eax, RegisterExpectation::Exact(1)),
        (Eax, RegisterExpectation::Exact(2)),
    ];
    rejects(|| validate(&fixture.case), "duplicate register Eax");
    fixture.case.expected.registers = vec![];
    fixture.case.expected.memory = vec![
        MemoryExpectation::Exact {
            address: 0x4000,
            bytes: vec![0xa5, 0],
        },
        MemoryExpectation::Undefined {
            address: 0x4001,
            length: 1,
        },
    ];
    rejects(|| validate(&fixture.case), "overlapping memory spans");
}

#[test]
fn opaque_flag_replacement_cannot_claim_unspecified_bits_are_preserved() {
    let case = InstructionCase::replacing_flags(
        "comparison contract",
        &[0x04, 1],
        Flags::all(FlagExpectation::Preserved),
    );
    rejects(
        || validate(&case),
        "replacing opaque flags requires explicit results",
    );
}

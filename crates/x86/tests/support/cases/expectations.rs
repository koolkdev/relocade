//! Case validation and comparisons; instruction results are supplied by the case.

#[cfg(test)]
mod tests;

use std::ops::Range;

use crate::flags::Flag;
use wasm86_x86::Gpr32;

use super::{
    ExpectedExit, ExpectedFlags, ExpectedState, FlagExpectation, Flags, InstructionCase,
    MemoryExpectation, RegisterExpectation,
};
use crate::support::guest::{Execution, Exit, State};

pub(super) fn validate(case: &InstructionCase) {
    assert!(!case.name.is_empty(), "an instruction case needs a name");
    assert!(
        !case.code.is_empty() && case.code.len() <= 15,
        "{}: a case contains one instruction of at most fifteen bytes",
        case.name
    );
    if let (
        None,
        ExpectedFlags::Logical {
            values: expected, ..
        },
    ) = (case.initial.flags.logical(), case.expected.flags)
    {
        assert!(
            !expected
                .values()
                .iter()
                .any(|rule| matches!(rule, FlagExpectation::Preserved)),
            "{}: replacing opaque flags requires explicit results or Undefined",
            case.name
        );
    }
    validate_initial(&case.name, &case.initial);
    validate_expected(&case.name, &case.expected);
}

pub(in crate::support) fn validate_initial(name: &str, initial: &super::InitialState) {
    unique_registers(
        name,
        initial.registers.iter().map(|&(register, _)| register),
    );
    disjoint(
        name,
        initial
            .memory
            .iter()
            .map(|region| span(name, region.address, region.bytes.len() as u64)),
    );
}

pub(in crate::support) fn validate_expected(name: &str, expected: &ExpectedState) {
    unique_registers(
        name,
        expected.registers.iter().map(|&(register, _)| register),
    );
    disjoint(
        name,
        expected.memory.iter().map(|memory| match memory {
            MemoryExpectation::Exact { address, bytes } => span(name, *address, bytes.len() as u64),
            MemoryExpectation::Undefined { address, length } => {
                span(name, *address, u64::from(*length))
            }
        }),
    );
}

fn unique_registers(name: &str, registers: impl Iterator<Item = Gpr32>) {
    let registers = registers.collect::<Vec<_>>();
    for (index, register) in registers.iter().enumerate() {
        assert!(
            !registers[..index].contains(register),
            "{name}: duplicate register {register:?}"
        );
    }
}

fn span(name: &str, address: u32, length: u64) -> Range<u64> {
    let start = u64::from(address);
    let end = start + length;
    assert!(
        length != 0 && end <= 1_u64 << 32,
        "{name}: invalid memory span"
    );
    start..end
}

fn disjoint(name: &str, ranges: impl Iterator<Item = Range<u64>>) {
    let ranges = ranges.collect::<Vec<_>>();
    for (index, range) in ranges.iter().enumerate() {
        assert!(
            ranges[..index]
                .iter()
                .all(|earlier| earlier.end <= range.start || range.end <= earlier.start),
            "{name}: overlapping memory spans"
        );
    }
}

pub(super) fn check_state(
    case: &InstructionCase,
    initial: &State,
    execution: &Execution,
    context: &str,
) {
    check_checkpoint(
        &case.expected,
        Boundary {
            eip: case.expected_eip(),
            retired: case.expected_retired(),
        },
        initial,
        execution,
        context,
    );
}

#[derive(Clone, Copy)]
pub(in crate::support) struct Boundary {
    pub(in crate::support) eip: u32,
    pub(in crate::support) retired: u32,
}

pub(in crate::support) fn check_checkpoint(
    expected: &ExpectedState,
    boundary: Boundary,
    initial: &State,
    execution: &Execution,
    context: &str,
) {
    let expected_eip = boundary.eip;
    let actual = &execution.state;
    for register in Gpr32::ALL {
        let rule = expected
            .registers
            .iter()
            .find(|(name, _)| *name == register);
        let (value, mask) = match rule.map(|(_, rule)| rule) {
            Some(RegisterExpectation::Exact(value)) => (*value, u32::MAX),
            Some(RegisterExpectation::DefinedBits { value, mask }) => (*value, *mask),
            None => (initial.cpu.registers[register], u32::MAX),
        };
        let actual = actual.cpu.registers[register];
        assert!(
            actual & mask == value & mask,
            "{context}: {register:?} expected {value:08x}, actual {actual:08x}, defined mask {mask:08x}"
        );
    }
    assert_eq!(actual.cpu.eip, expected_eip, "{context}: EIP");
    assert_eq!(
        actual.cpu.instruction_count,
        initial.cpu.instruction_count.wrapping_add(boundary.retired),
        "{context}: retired instruction count"
    );
    assert_eq!(
        actual.cpu.segments, initial.cpu.segments,
        "{context}: loaded segment registers"
    );
    assert_eq!(
        actual.cpu.reserved, initial.cpu.reserved,
        "{context}: reserved CPU bytes"
    );
    assert_eq!(
        actual.cpu.reserved_tail, initial.cpu.reserved_tail,
        "{context}: reserved CPU tail"
    );
    let mut expected_record = initial.cpu.flags;
    for &(flag, value) in &expected.direct_flags {
        let byte = match flag {
            Flag::TF => &mut expected_record.bytes.tf,
            Flag::DF => &mut expected_record.bytes.df,
            Flag::NT => &mut expected_record.bytes.nt,
            Flag::AC => &mut expected_record.bytes.ac,
            Flag::ID => &mut expected_record.bytes.id,
            _ => unreachable!("only direct flags enter this expectation list"),
        };
        *byte = u8::from(value);
    }
    assert_eq!(
        [
            actual.cpu.flags.bytes.tf,
            actual.cpu.flags.bytes.df,
            actual.cpu.flags.bytes.nt,
            actual.cpu.flags.bytes.ac,
            actual.cpu.flags.bytes.id,
            actual.cpu.flags.bytes.reserved
        ],
        [
            expected_record.bytes.tf,
            expected_record.bytes.df,
            expected_record.bytes.nt,
            expected_record.bytes.ac,
            expected_record.bytes.id,
            expected_record.bytes.reserved
        ],
        "{context}: control and system flags and reserved byte"
    );
    assert_eq!(
        actual.cpu.flags.status_source.reserved, initial.cpu.flags.status_source.reserved,
        "{context}: reserved flag bytes"
    );
    if expected.flags.preserves_record() {
        assert_eq!(
            actual.cpu.flags, expected_record,
            "{context}: stored flag record"
        );
    }
    let mut memory = initial.memory.clone();
    for expectation in &expected.memory {
        match expectation {
            MemoryExpectation::Exact { address, bytes } => memory.write(*address, bytes),
            MemoryExpectation::Undefined { address, length } => {
                memory.write(*address, &actual.memory.read(*address, *length as usize));
            }
        }
    }
    assert_eq!(
        actual.memory, memory,
        "{context}: guest memory, including unchanged bytes"
    );
    assert!(
        execution.machine_unchanged,
        "{context}: page mappings changed"
    );
    if matches!(
        expected.exit,
        ExpectedExit::DivideError
            | ExpectedExit::BoundRangeExceeded
            | ExpectedExit::InvalidOpcode
            | ExpectedExit::GeneralProtection { .. }
            | ExpectedExit::StackFault { .. }
            | ExpectedExit::PageFault { .. }
    ) {
        assert!(
            execution.dispatches.is_empty(),
            "{context}: a fault must not dispatch"
        );
    }
    let exit = match expected.exit {
        ExpectedExit::Fallthrough | ExpectedExit::Dispatch(_) => {
            assert_eq!(
                execution.dispatches,
                [(expected_eip, actual.clone())],
                "{context}: dispatch must expose the completed state"
            );
            Exit::Dispatch(expected_eip)
        }
        ExpectedExit::DivideError => Exit::DivideError,
        ExpectedExit::BoundRangeExceeded => Exit::BoundRangeExceeded,
        ExpectedExit::InvalidOpcode => Exit::InvalidOpcode,
        ExpectedExit::GeneralProtection { error } => Exit::GeneralProtection { error },
        ExpectedExit::StackFault { error } => Exit::StackFault { error },
        ExpectedExit::PageFault { address, error } => Exit::PageFault { address, error },
    };
    assert_eq!(execution.exit, exit, "{context}: exit");
}

pub(super) fn check_flags(case: &InstructionCase, actual: Flags<bool>, context: &str) {
    let ExpectedFlags::Logical {
        values: expected, ..
    } = case.expected.flags
    else {
        panic!("{context}: opaque flag records are compared as bytes");
    };
    check_flag_values(case.initial.flags.logical(), expected, actual, context);
}

pub(in crate::support) fn check_flag_values(
    initial: Option<Flags<bool>>,
    expected: Flags<FlagExpectation>,
    actual: Flags<bool>,
    context: &str,
) {
    let initial = initial.map(Flags::values);
    let actual = actual.values();
    for (index, (name, rule)) in ["CF", "PF", "AF", "ZF", "SF", "OF"]
        .into_iter()
        .zip(expected.values())
        .enumerate()
    {
        let expected = match rule {
            FlagExpectation::Set => true,
            FlagExpectation::Clear => false,
            FlagExpectation::Preserved => {
                initial.expect("preservation requires logical initial flags")[index]
            }
            FlagExpectation::Undefined => continue,
        };
        assert_eq!(actual[index], expected, "{context}: {name} ({rule:?})");
    }
}

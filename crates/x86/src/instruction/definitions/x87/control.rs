//! x87 environment and coprocessor controls.
//! Control words have fixed widths, independent of operand-size prefixes.

use super::*;

instruction_families! {
    FNOP {
        execute: no_operation;
        requires: [Fpu];
        forms { 0xD9 @ 0xD0 => no_operands(); }
    }
    FNENI {
        execute: legacy_control;
        requires: [Fpu];
        forms { 0xDB @ 0xE0 => no_operands(); }
    }
    FNDISI {
        execute: legacy_control;
        requires: [Fpu];
        forms { 0xDB @ 0xE1 => no_operands(); }
    }
    FSETPM {
        execute: legacy_control;
        requires: [Fpu];
        forms { 0xDB @ 0xE4 => no_operands(); }
    }
    FNINIT {
        execute: initialize;
        requires: [Fpu];
        forms { 0xDB @ 0xE3 => no_operands(); }
    }
    FNCLEX {
        execute: clear_exceptions;
        requires: [Fpu];
        forms { 0xDB @ 0xE2 => no_operands(); }
    }
    FWAIT {
        execute: check_pending_exception;
        requires: [Fpu];
        forms { 0x9B => no_operands(); }
    }
    FLDCW {
        execute: load_control;
        requires: [Fpu];
        forms { 0xD9 / 5 => operands(mem16); }
    }
    FNSTCW {
        execute: store_control;
        requires: [Fpu];
        forms { 0xD9 / 7 => operands(mem16); }
    }
    FNSTSW {
        execute: store_status;
        requires: [Fpu];
        forms {
            0xDD / 7 => operands(mem16);
            0xDF @ 0xE0 => operands(AX);
        }
    }
}

fn no_operation(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()
}

// P4 ignores these legacy controls without checking pending exceptions or
// updating x87 instruction metadata.
fn legacy_control(_execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    Ok(())
}

fn check_pending_exception(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()
}

fn initialize(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.x87().initialize()
}

fn clear_exceptions(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.x87().clear_exceptions()
}

fn load_control(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: TypedLocation<I16>,
) -> Result<(), BuildError> {
    // The emulator resolves an already pending exception before the operand
    // access. No new control or summary bits commit if that access faults.
    execution.check_x87_exception()?;
    let control = source.read(execution)?;
    execution.x87().load_control_word(control)
}

fn store_control(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let control = execution.x87().control_word()?;
    destination.write(execution, control)
}

fn store_status(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I16>,
) -> Result<(), BuildError> {
    let status = execution.x87().status_word()?;
    destination.write(execution, status)
}

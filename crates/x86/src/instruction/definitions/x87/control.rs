//! x87 control words have fixed widths, independent of operand-size prefixes.

use super::*;

instruction_families! {
    FNINIT {
        execute: initialize;
        forms { 0xDB @ 0xE3 => no_operands(); }
    }
    FNCLEX {
        execute: clear_exceptions;
        forms { 0xDB @ 0xE2 => no_operands(); }
    }
    FWAIT {
        execute: check_pending_exception;
        forms { 0x9B => no_operands(); }
    }
    FLDCW {
        execute: load_control;
        forms { 0xD9 / 5 => operands(mem16); }
    }
    FNSTCW {
        execute: store_control;
        forms { 0xD9 / 7 => operands(mem16); }
    }
    FNSTSW {
        execute: store_status;
        forms {
            0xDD / 7 => operands(mem16);
            0xDF @ 0xE0 => operands(AX);
        }
    }
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

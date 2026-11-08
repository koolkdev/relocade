//! Constructs guest entry frames for interrupts and far calls.

use crate::{runtime::Machine, FarPointer};
use relocade_x86::{ExecutionExit, Segment};

pub(in crate::runtime) enum GuestCall {
    Interrupt(u8),
    Far {
        target: FarPointer,
        arguments: Box<[u16]>,
    },
}

impl GuestCall {
    /// Builds the complete entry frame before the driver dispatches guest code.
    pub(super) fn enter(&self, machine: &mut Machine) -> Result<(), ExecutionExit> {
        match self {
            Self::Interrupt(vector) => match machine
                .dispatch
                .enter_interrupt(&mut machine.backend, *vector)
            {
                ExecutionExit::InterruptEntered => Ok(()),
                exit => Err(exit),
            },
            Self::Far { target, arguments } => {
                let stack_bytes = size_of_val(arguments.as_ref()) as u16;
                let exit =
                    machine
                        .dispatch
                        .enter_far_call(&mut machine.backend, *target, stack_bytes);
                if exit != ExecutionExit::FarCallEntered {
                    return Err(exit);
                }
                // Entry checked the entire frame. Fill its reserved words in
                // push order through the guest map before any guest code runs.
                let mut call = machine.context();
                let stack = call.cpu.segment(Segment::Ss);
                let mut offset =
                    u32::from(call.cpu.sp()) + FarPointer::BYTE_LEN as u32 + u32::from(stack_bytes);
                for argument in arguments {
                    offset -= size_of::<u16>() as u32;
                    call.memory
                        .write_segment(&stack, offset, &argument.to_le_bytes())
                        .expect("far-call entry checked the argument frame");
                }
                Ok(())
            }
        }
    }
}

//! Constructs guest entry frames for interrupts and far calls.

use crate::{runtime::CpuInstance, FarPointer};
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
    pub(super) fn enter(&self, instance: &mut CpuInstance) -> Result<(), ExecutionExit> {
        match self {
            Self::Interrupt(vector) => match instance
                .runtime
                .with_runtime(|wasm| instance.dispatch.enter_interrupt(wasm, *vector))
            {
                ExecutionExit::InterruptEntered => Ok(()),
                exit => Err(exit),
            },
            Self::Far { target, arguments } => {
                let stack_bytes = size_of_val(arguments.as_ref()) as u16;
                let exit = instance.runtime.with_runtime(|wasm| {
                    instance.dispatch.enter_far_call(wasm, *target, stack_bytes)
                });
                if exit != ExecutionExit::FarCallEntered {
                    return Err(exit);
                }
                // Entry checked the entire frame. Fill its reserved words in
                // push order through the guest map before any guest code runs.
                let mut call = instance.context();
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

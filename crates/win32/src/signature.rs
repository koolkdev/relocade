use crate::{Arguments, Exception, GuestMemory, ReturnValue, StackReader};
use relocade_x86::CpuState;
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallingConvention {
    Stdcall,
    Cdecl,
}

/// One function type used by export bindings and outgoing near calls.
pub struct Signature<A, R> {
    convention: CallingConvention,
    types: PhantomData<fn(A) -> R>,
}

impl<A: Arguments, R: ReturnValue> Signature<A, R> {
    pub const fn new(convention: CallingConvention) -> Self {
        if let CallingConvention::Stdcall = convention {
            assert!(
                A::STACK_BYTES <= u16::MAX as u32,
                "stdcall cleanup must fit RET imm16"
            );
        }
        Self {
            convention,
            types: PhantomData,
        }
    }

    pub const fn convention(&self) -> CallingConvention {
        self.convention
    }

    /// Guest bytes for a direct host entry, a suspended-call resume position
    /// and the near return executed by the guest after normal completion.
    pub fn stub(&self) -> Vec<u8> {
        match self.convention {
            CallingConvention::Stdcall if A::STACK_BYTES != 0 => {
                let [low, high] = (A::STACK_BYTES as u16).to_le_bytes();
                vec![0x90, 0x90, 0xc2, low, high]
            }
            _ => vec![0x90, 0x90, 0xc3],
        }
    }

    /// Decodes fixed parameters and leaves the cursor at the first unnamed slot.
    pub fn arguments(&self, stack: &mut StackReader<'_>) -> Result<A, Exception> {
        A::read(stack)
    }

    /// Enters a guest callback on a stopped Flat32 CPU. Its live EIP supplies
    /// the caller's resume address. A stack fault leaves EIP and ESP intact.
    pub fn enter_callback(
        &self,
        cpu: &mut CpuState,
        memory: &dyn GuestMemory,
        target: u32,
        arguments: &A,
    ) -> Result<Callback<R>, Exception> {
        let frame_bytes = A::STACK_BYTES
            .checked_add(4)
            .expect("near-call frame fits 32 bits");
        let mut frame = Vec::with_capacity(frame_bytes as usize);
        frame.extend_from_slice(&cpu.eip.to_le_bytes());
        arguments.encode(&mut frame);
        assert_eq!(
            frame.len(),
            frame_bytes as usize,
            "encoded parameter layout"
        );
        let esp = cpu.registers.esp.wrapping_sub(frame_bytes);
        memory.write(esp, &frame)?;
        cpu.registers.esp = esp;
        cpu.eip = target;
        let caller_cleanup = match self.convention {
            CallingConvention::Stdcall => 0,
            CallingConvention::Cdecl => A::STACK_BYTES,
        };
        Ok(Callback {
            caller_cleanup,
            result: PhantomData,
        })
    }
}

/// Owned by a single callback invocation. Complete after its guest RET reaches
/// the resume position. Abandoning a callback leaves the live CPU untouched.
#[must_use]
pub struct Callback<R> {
    caller_cleanup: u32,
    result: PhantomData<R>,
}

impl<R: ReturnValue> Callback<R> {
    pub fn complete(self, cpu: &mut CpuState) -> R {
        cpu.registers.esp = cpu.registers.esp.wrapping_add(self.caller_cleanup);
        R::from_registers(cpu.registers.eax, cpu.registers.edx)
    }
}

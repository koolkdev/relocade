mod address;
mod control;
mod memory;
mod operands;
mod ports;
mod regions;
mod register_pairs;
mod segments;
mod stack;
mod strings;
mod x87;

pub(crate) use control::CodeTarget;
pub(crate) use operands::WriteTarget;
pub(crate) use strings::{ResolvedStrings, StringOperand};

use wasm86_compiler::{BlockBuilder, BuildError, Func, Val, I1, I16, I32, I8};

use crate::flags::{Condition, Flag, FlagChange};
use crate::instruction::{self, DecodedInstruction, SegmentOverride};
use crate::memory::{Accesses, Memory};
use crate::runtime::Runtime;
use crate::segment::SegmentAccess;
use crate::state::{exit, Cpu, State};
use crate::{address::AddressSize, exception::Exception, CpuState, ExecutionProfile};

/// Builds one execution path. State definitions and progress describe completed
/// instructions. Instructions with partial progress, such as REP and POPA, also
/// define completed effects while EIP and the instruction count stay at entry.
pub(super) struct ExecutionBuilder<'body, 'module> {
    body: BlockBuilder<'body>,
    state: State<'module>,
    memory: Option<Accesses<'module>>,
    segments: SegmentAccess<'module>,
    segment_override: SegmentOverride,
    address_size: AddressSize,
    locked: bool,
    x87_opcode: Option<Val<I16>>,
    runtime: Runtime,
    eip: Val<I32>,
    completed: u32,
    work: Option<Val<I32>>,
    resume_instruction: Option<Func>,
    repetition_accounts_work: bool,
    can_specialize: bool,
    observed_cpu: Option<&'module CpuState>,
}

impl<'body, 'module> ExecutionBuilder<'body, 'module> {
    pub(super) fn new(
        mut body: BlockBuilder<'body>,
        cpu: &'module Cpu,
        memory: Option<&'module Memory>,
        runtime: Runtime,
        start: impl Into<Val<I32>>,
        profile: ExecutionProfile,
    ) -> Result<Self, BuildError> {
        let eip = body.value(start)?;
        let work = runtime.remaining_work(&mut body)?;
        Ok(Self {
            body,
            state: State::new(cpu),
            memory: memory.map(Memory::accesses),
            segments: SegmentAccess::new(cpu, profile),
            segment_override: SegmentOverride::None,
            address_size: AddressSize::Bits32,
            locked: false,
            x87_opcode: None,
            runtime,
            eip,
            completed: 0,
            work,
            resume_instruction: None,
            repetition_accounts_work: false,
            can_specialize: false,
            observed_cpu: None,
        })
    }

    /// Allows a compiled block to abandon speculation at an instruction boundary.
    pub(super) fn with_specialization(mut self, observed_cpu: Option<&'module CpuState>) -> Self {
        self.can_specialize = true;
        self.observed_cpu = observed_cpu;
        self
    }

    pub(super) fn with_instruction_resume(mut self, entry: Func) -> Self {
        self.resume_instruction = Some(entry);
        self
    }

    /// Executes an instruction whose required bytes have passed fetch checks.
    pub(super) fn execute<V: Into<Val<I32>>, P: Into<Val<I32>>>(
        &mut self,
        mut decoded: DecodedInstruction<V, P>,
    ) -> Result<(), BuildError> {
        self.eip = self.body.value(decoded.eip)?;
        self.repetition_accounts_work = false;
        self.check_budget()?;
        let fallthrough_eip = self.body.value(decoded.fallthrough_eip)?;
        self.address_size = decoded.instruction.address_size;
        self.segment_override = decoded.instruction.segment_override.clone();
        self.locked = decoded.instruction.locked;
        self.x87_opcode = decoded
            .instruction
            .x87_opcode
            .take()
            .map(|opcode| opcode.bits());
        self.eip = instruction::lower(self, decoded.instruction, fallthrough_eip)?;
        self.retire()?;
        Ok(())
    }

    /// Rejects an instruction before effects when its local slice budget is empty.
    fn check_budget(&mut self) -> Result<(), BuildError> {
        if let Some(work) = &self.work {
            self.body.if_(work.eq(0), |mut body| {
                self.runtime.publish_work(&mut body, Some(work))?;
                self.state.publish(&mut body, &self.eip, self.completed)?;
                body.return_(crate::SLICE_EXHAUSTED)
            })?;
        }
        Ok(())
    }

    pub(crate) fn consume_work(&mut self, units: impl Into<Val<I32>>) -> Result<(), BuildError> {
        if let Some(work) = &self.work {
            self.work = Some(self.body.value(work.sub(units))?);
        }
        Ok(())
    }

    pub(crate) fn is_budgeted(&self) -> bool {
        self.work.is_some()
    }

    fn retire(&mut self) -> Result<(), BuildError> {
        if !self.repetition_accounts_work {
            self.consume_work(1)?;
        }
        self.completed += 1;
        Ok(())
    }

    pub(crate) fn begin_repetition(&mut self, count: &Val<I32>) -> Result<(), BuildError> {
        self.repetition_accounts_work = true;
        self.consume_work(count.eq(0).unsigned().extend::<I32>())?;
        Ok(())
    }

    /// REP can leave a completed chunk at its current instruction boundary.
    /// Runtime decoding resumes internally; a snapshot abandons its suffix.
    pub(crate) fn repeat_again_if(&mut self, pending: Val<I1>) -> Result<(), BuildError> {
        self.body.if_(pending, |mut body| {
            self.runtime.publish_work(&mut body, self.work.as_ref())?;
            self.state.publish(&mut body, &self.eip, self.completed)?;
            if let Some(work) = &self.work {
                body.if_(work.eq(0), |body| body.return_(crate::SLICE_EXHAUSTED))?;
            }
            match self.resume_instruction {
                Some(entry) => body.tail_call(entry, &[]),
                None => self.runtime.dispatch(body, &self.eip),
            }
        })
    }

    /// Builds JIT guards, then refines local candidates for their continuation.
    /// Runtime decoding and nested regions skip the callback and retain the
    /// ordinary semantics. Keep current-instruction effects after this scope.
    pub(crate) fn specialize(
        &mut self,
        build: impl FnOnce(&mut Self) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        if self.can_specialize {
            build(self)
        } else {
            Ok(())
        }
    }

    /// Requires an assumption on a specializing path, before instruction effects.
    /// Failure publishes the current restart boundary and enters the interpreter;
    /// the continuing path can use the assumption for subsequent refinements.
    pub(crate) fn specialize_on(
        &mut self,
        condition: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        assert!(
            self.can_specialize,
            "this path does not permit specialization"
        );
        self.body.if_(condition.into().eq(false), |mut body| {
            self.runtime.publish_work(&mut body, self.work.as_ref())?;
            self.state.publish(&mut body, &self.eip, self.completed)?;
            self.runtime.interpret(body)
        })
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    pub(crate) fn profile(&self) -> ExecutionProfile {
        self.segments.profile()
    }

    /// Builds compiler values, including pure control-flow joins, in the current
    /// body. The callback must leave it open and must not change guest state or
    /// memory. Use execution regions for branches with architectural effects.
    pub(crate) fn compute<R>(
        &mut self,
        build: impl FnOnce(&mut BlockBuilder<'body>) -> Result<R, BuildError>,
    ) -> Result<R, BuildError> {
        build(&mut self.body)
    }

    /// Defines a flag change while preserving flags omitted from its write mask.
    pub(super) fn write_flags(&mut self, change: impl Into<FlagChange>) -> Result<(), BuildError> {
        self.state.write_flags(&mut self.body, change)
    }

    pub(crate) fn read_iopl(&mut self) -> Result<Val<I8>, BuildError> {
        self.state.read_iopl(&mut self.body)
    }

    pub(crate) fn write_iopl(&mut self, value: Val<I8>) -> Result<(), BuildError> {
        self.state.write_iopl(&mut self.body, value)
    }

    pub(super) fn read_flag(&mut self, flag: Flag) -> Result<Val<I1>, BuildError> {
        self.state.read_flag(&mut self.body, flag)
    }

    /// Reads current logical flags in request order, resolving shared backing together.
    pub(super) fn read_flags<const N: usize>(
        &mut self,
        flags: [Flag; N],
    ) -> Result<[Val<I1>; N], BuildError> {
        self.state.read_flags(&mut self.body, flags)
    }

    pub(super) fn write_flag(
        &mut self,
        flag: Flag,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.state.write_flag(&mut self.body, flag, value)
    }

    pub(super) fn condition(&mut self, condition: Condition) -> Result<Val<I1>, BuildError> {
        self.state.condition(&mut self.body, condition)
    }

    /// Raises a guest fault at the current instruction's restart boundary.
    /// Publishes completed work without retiring the faulting instruction.
    pub(crate) fn fault(&mut self, exception: Exception<Val<I32>>) -> Result<(), BuildError> {
        self.fault_if(true, exception)
    }

    /// Ends a faulting path with the current state. Define only effects permitted
    /// to survive that fault; the instruction's EIP and retirement stay at entry.
    pub(crate) fn fault_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        exception: Exception<Val<I32>>,
    ) -> Result<(), BuildError> {
        self.body.if_(condition, |mut fault_body| {
            self.runtime
                .publish_work(&mut fault_body, self.work.as_ref())?;
            self.state
                .fault(fault_body, &self.eip, self.completed, exception)
        })
    }

    /// Stops unsupported execution at the current instruction's restart boundary.
    pub(crate) fn unsupported(&mut self, opcode: impl Into<Val<I8>>) -> Result<(), BuildError> {
        self.unsupported_if(true, opcode)
    }

    /// Stops an unsupported execution path without retiring the instruction.
    /// Earlier completed work is published at the current restart boundary.
    /// Call before defining any effects of the current instruction.
    pub(crate) fn unsupported_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        opcode: impl Into<Val<I8>>,
    ) -> Result<(), BuildError> {
        self.body.if_(condition, |mut body| {
            self.runtime.publish_work(&mut body, self.work.as_ref())?;
            self.state.publish(&mut body, &self.eip, self.completed)?;
            exit::unsupported(body, &self.eip, opcode)
        })
    }

    /// Publishes completed work before the frontend dispatches or continues decoding.
    pub(super) fn complete(
        mut self,
        continue_execution: impl FnOnce(BlockBuilder<'body>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.runtime
            .publish_work(&mut self.body, self.work.as_ref())?;
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        continue_execution(self.body, &self.eip)
    }
}

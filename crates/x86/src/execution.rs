mod address;
mod control;
mod memory;
mod mmx;
mod operands;
mod ports;
mod regions;
mod register_pairs;
mod segments;
mod simd;
mod stack;
mod strings;
mod x87;

pub(crate) use control::CodeTarget;
pub(crate) use memory::{MemoryOperand, OperandSpan};
pub(crate) use operands::WriteTarget;
pub(crate) use strings::{ResolvedStrings, StringOperand};
pub(crate) use x87::{X87MemoryFormat, X87Operand, X87Operands};

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I64, I8};

use crate::flags::{Condition, Flag, FlagChange};
use crate::instruction::{self, DecodedInstruction, SegmentOverride};
use crate::memory::{Access, Accesses, Intent, Memory};
use crate::runtime::Runtime;
use crate::segment::SegmentSelection;
use crate::state::{exit, Cpu, SegmentReads, State};
use crate::{
    address::AddressSize, exception::Exception, CpuState, ExecutionProfile, ExecutionTarget,
};

/// Builds one execution path. State definitions and progress describe completed
/// instructions. Instructions with partial progress, such as REP and POPA, also
/// define completed effects while EIP and the instruction count stay at entry.
pub(super) struct ExecutionBuilder<'body, 'module> {
    body: BlockBuilder<'body>,
    state: State<'module>,
    memory: Option<Accesses<'module>>,
    segments: SegmentReads<'module>,
    target: ExecutionTarget,
    segment_override: SegmentOverride,
    address_size: AddressSize,
    locked: bool,
    x87_opcode: Option<Val<I16>>,
    runtime: Runtime,
    eip: Val<I32>,
    completed: u32,
    can_specialize: bool,
    observed_cpu: Option<&'module CpuState>,
}

impl<'body, 'module> ExecutionBuilder<'body, 'module> {
    pub(super) fn new(
        body: BlockBuilder<'body>,
        cpu: &'module Cpu,
        memory: Option<&'module Memory>,
        runtime: Runtime,
        start: impl Into<Val<I32>>,
        target: ExecutionTarget,
    ) -> Result<Self, BuildError> {
        let eip = body.value(start)?;
        Ok(Self {
            body,
            state: State::new(cpu),
            memory: memory.map(Memory::accesses),
            segments: SegmentReads::new(cpu, target.profile),
            target,
            segment_override: SegmentOverride::None,
            address_size: AddressSize::Bits32,
            locked: false,
            x87_opcode: None,
            runtime,
            eip,
            completed: 0,
            can_specialize: false,
            observed_cpu: None,
        })
    }

    /// Enables guarded optimizations and interpreter handoff for a compiled block.
    pub(super) fn with_specialization(mut self, observed_cpu: Option<&'module CpuState>) -> Self {
        self.can_specialize = true;
        self.observed_cpu = observed_cpu;
        self
    }

    /// Executes an instruction whose required bytes have passed fetch checks.
    pub(super) fn execute<V: Into<Val<I32>>, P: Into<Val<I32>>>(
        &mut self,
        mut decoded: DecodedInstruction<V, P>,
    ) -> Result<(), BuildError> {
        self.eip = self.body.value(decoded.eip)?;
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
        self.completed += 1;
        Ok(())
    }

    /// Builds JIT guards, then refines local candidates for their continuation.
    /// Runtime decoding and nested regions skip the callback and retain the
    /// ordinary semantics. Keep current-instruction effects after this scope.
    pub(crate) fn specialize<T>(
        &mut self,
        build: impl FnOnce(&mut Self) -> Result<T, BuildError>,
    ) -> Result<Option<T>, BuildError> {
        if self.can_specialize {
            build(self).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Excludes these conditions from compiled execution and clears their local
    /// evidence only after the guard succeeds. Other execution paths retain it.
    pub(crate) fn specialize_without<const N: usize>(
        &mut self,
        conditions: [&mut Val<I1>; N],
    ) -> Result<(), BuildError> {
        self.specialize(|jit| {
            let excluded = conditions
                .iter()
                .fold(Val::<I1>::from(false), |excluded, condition| {
                    excluded.or(&**condition)
                });
            jit.guard(excluded.eq(false))?;
            for condition in conditions {
                *condition = false.into();
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Builds a numerical calculation in the current compiler body. The callback
    /// must not author architectural effects or instruction exits.
    pub(crate) fn calculate<T>(
        &mut self,
        build: impl FnOnce(&mut BlockBuilder<'body>) -> Result<T, BuildError>,
    ) -> Result<T, BuildError> {
        build(&mut self.body)
    }

    /// Requires a condition for compiled execution, before instruction effects.
    /// Failure publishes the current restart boundary and enters the interpreter.
    /// Specialization may use the proven condition to refine subsequent values.
    pub(crate) fn guard(&mut self, condition: impl Into<Val<I1>>) -> Result<(), BuildError> {
        assert!(
            self.can_specialize,
            "this path does not permit interpreter handoff"
        );
        self.body.if_(condition.into().eq(false), |mut body| {
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

    pub(crate) fn target(&self) -> ExecutionTarget {
        self.target
    }

    /// Retired instructions, including this entry's unpublished progress.
    pub(crate) fn instruction_count(&mut self) -> Result<Val<I64>, BuildError> {
        self.state.instruction_count(&mut self.body, self.completed)
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
        self.body.if_(condition, |fault_body| {
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
            self.state.publish(&mut body, &self.eip, self.completed)?;
            exit::unsupported(body, &self.eip, opcode)
        })
    }

    fn checked(
        &mut self,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        span: impl Into<OperandSpan>,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        let span = span.into();
        let linear = self.translate(segment, offset, span.bytes, intent)?;
        self.fault_if(
            linear.and(span.alignment - 1).ne(0),
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
        )?;
        self.resolve_access(&linear, span.bytes, intent)
    }

    fn translate(
        &mut self,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Val<I32>, BuildError> {
        self.segments.translate(
            &mut self.body,
            segment,
            offset,
            bytes,
            intent,
            |fault_body, exception| {
                self.state
                    .fault(fault_body, &self.eip, self.completed, exception)
            },
        )
    }

    fn resolve_access(
        &mut self,
        linear: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        self.memory
            .as_mut()
            .expect("a memory access declares guest memory")
            .resolve(
                &mut self.body,
                linear,
                bytes,
                intent,
                |fault_body, exception| {
                    self.state
                        .fault(fault_body, &self.eip, self.completed, exception)
                },
            )
    }

    /// Ends a block, checks its instruction target and dispatches its successor.
    pub(super) fn dispatch(self) -> Result<(), BuildError> {
        let runtime = self.runtime;
        self.finish(true, |body, eip| runtime.dispatch(body, eip))
    }

    /// Publishes an instruction and continues decoding within the current block.
    pub(super) fn complete(
        self,
        continue_execution: impl FnOnce(BlockBuilder<'body>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.finish(false, continue_execution)
    }

    fn finish(
        mut self,
        check_budget: bool,
        continue_execution: impl FnOnce(BlockBuilder<'body>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let count = self
            .state
            .publish(&mut self.body, &self.eip, self.completed)?
            .expect("successful completion retires an instruction");
        if check_budget {
            self.state
                .check_instruction_budget(&mut self.body, &count)?;
        }
        continue_execution(self.body, &self.eip)
    }
}

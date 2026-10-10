mod address;
mod control;
mod memory;
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
pub(crate) use memory::OperandSpan;
pub(crate) use operands::WriteTarget;
pub(crate) use strings::{ResolvedStrings, StringOperand};
pub(crate) use x87::X87Operand;

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I8};

use crate::flags::{Condition, Flag, FlagChange};
use crate::instruction::{self, DecodedInstruction, SegmentOverride};
use crate::memory::{Access, Accesses, Intent, Memory};
use crate::runtime::Runtime;
use crate::segment::{SegmentAccess, SegmentSelection};
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
        profile: ExecutionProfile,
    ) -> Result<Self, BuildError> {
        let eip = body.value(start)?;
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

    /// Publishes completed work before the frontend dispatches or continues decoding.
    pub(super) fn complete(
        mut self,
        continue_execution: impl FnOnce(BlockBuilder<'body>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        continue_execution(self.body, &self.eip)
    }
}

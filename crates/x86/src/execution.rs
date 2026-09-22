mod address;
mod control;
mod memory;
mod operands;
mod regions;
mod register_pairs;
mod segments;
mod stack;
pub(crate) mod x87;

pub(crate) use control::CodeTarget;
pub(crate) use operands::WriteTarget;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I16, I32, I8};

use crate::flags::{Condition, Flag, FlagChange};
use crate::instruction::{self, DecodedInstruction, SegmentOverride};
use crate::memory::{Access, Intent, Memory};
use crate::runtime::Runtime;
use crate::segment::{SegmentAccess, SegmentProfile, SegmentSelection};
use crate::state::{exit, Cpu, State};
use crate::{address::AddressSize, exception::Exception};

/// Builds one execution path. State definitions and progress describe completed
/// instructions. Instructions with partial progress, such as REP and POPA, also
/// define completed effects while EIP and the instruction count stay at entry.
pub(super) struct ExecutionBuilder<'body, 'module> {
    body: FunctionBuilder<'body>,
    state: State<'module>,
    memory: Option<&'module Memory>,
    segments: SegmentAccess<'module>,
    segment_override: SegmentOverride,
    address_size: AddressSize,
    locked: bool,
    x87_opcode: Option<Val<I16>>,
    runtime: Runtime,
    eip: Val<I32>,
    completed: u32,
}

impl<'body, 'module> ExecutionBuilder<'body, 'module> {
    pub(super) fn new(
        body: FunctionBuilder<'body>,
        cpu: &'module Cpu,
        memory: Option<&'module Memory>,
        runtime: Runtime,
        start: impl Into<Val<I32>>,
        profile: SegmentProfile,
    ) -> Result<Self, BuildError> {
        let eip = body.value(start)?;
        Ok(Self {
            body,
            state: State::new(cpu),
            memory,
            segments: SegmentAccess::new(cpu, profile),
            segment_override: SegmentOverride::None,
            address_size: AddressSize::Bits32,
            locked: false,
            x87_opcode: None,
            runtime,
            eip,
            completed: 0,
        })
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

    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    /// Defines a flag change while preserving flags omitted from its write mask.
    pub(super) fn write_flags(&mut self, change: impl Into<FlagChange>) -> Result<(), BuildError> {
        self.state.write_flags(&mut self.body, change)
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
        memory: &'module Memory,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        let linear = self.translate(segment, offset, bytes, intent)?;
        self.resolve_access(memory, &linear, bytes, intent)
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
        memory: &Memory,
        linear: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        memory.resolve_access(
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
        continue_execution: impl FnOnce(FunctionBuilder<'body>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        continue_execution(self.body, &self.eip)
    }
}

//! Architectural environment images, distinct from the host snapshot layout.

use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I16, I32};

mod fx;
mod legacy;

use super::X87Access;
use crate::state::access::cpu_location;
use crate::x87::ExtendedBits;
use crate::SegmentDefaultSize;

struct Pointer {
    offset: Val<I32>,
    selector: Val<I16>,
}

struct Environment {
    control: Val<I16>,
    status: Val<I16>,
    tags: Val<I16>,
    /// The protected 14-byte image has no opcode field.
    opcode: Option<Val<I16>>,
    instruction: Pointer,
    data: Pointer,
}

#[derive(Clone, Copy)]
pub(crate) enum EnvironmentMode {
    Protected,
    Real,
}

impl X87Access<'_, '_> {
    /// Seven words or dwords in Intel's protected- or real-address-mode layout.
    /// Reserved halfwords are stored as ones and ignored when loading.
    pub(crate) fn environment<T: MemoryInt>(
        &mut self,
        mode: EnvironmentMode,
    ) -> Result<[Val<T>; 7], BuildError>
    where
        I32: AtLeast<T>,
    {
        let tags = self.state.registers.saved_tag_word(self.body)?;
        Ok(self.capture_environment(tags)?.encode(mode))
    }

    /// FXSAVE retains only occupancy, so its tags need no payload classification.
    pub(crate) fn fx_environment(
        &mut self,
        pointer_size: SegmentDefaultSize,
    ) -> Result<[Val<I32>; 6], BuildError> {
        let tags = self.state.registers.tag_word(self.body)?;
        Ok(self.capture_environment(tags)?.encode_fx(pointer_size))
    }

    fn capture_environment(&mut self, tags: Val<I16>) -> Result<Environment, BuildError> {
        Ok(Environment {
            control: self.control_word()?,
            status: self.status_word()?,
            tags,
            opcode: Some(
                self.state
                    .metadata
                    .read(self.body, cpu_location!(x87.opcode))?,
            ),
            instruction: Pointer {
                offset: self
                    .state
                    .metadata
                    .read(self.body, cpu_location!(x87.instruction_offset))?,
                selector: self
                    .state
                    .metadata
                    .read(self.body, cpu_location!(x87.instruction_selector))?,
            },
            data: Pointer {
                offset: self
                    .state
                    .metadata
                    .read(self.body, cpu_location!(x87.data_offset))?,
                selector: self
                    .state
                    .metadata
                    .read(self.body, cpu_location!(x87.data_selector))?,
            },
        })
    }

    /// Restores an environment and, when present, its eight logical stack values.
    /// Tags retain emptiness from the image and classify occupied payloads.
    pub(crate) fn restore<T: MemoryInt>(
        &mut self,
        mode: EnvironmentMode,
        words: &[Val<T>; 7],
        registers: Option<&[ExtendedBits; 8]>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        self.restore_environment(Environment::decode(words, mode), registers)
    }

    pub(crate) fn restore_fx(
        &mut self,
        pointer_size: SegmentDefaultSize,
        words: &[Val<I32>; 6],
        registers: &[ExtendedBits; 8],
    ) -> Result<(), BuildError> {
        self.restore_environment(Environment::decode_fx(words, pointer_size), Some(registers))
    }

    fn restore_environment(
        &mut self,
        environment: Environment,
        registers: Option<&[ExtendedBits; 8]>,
    ) -> Result<(), BuildError> {
        self.state
            .control
            .load_word(self.body, environment.control)?;
        self.state
            .status
            .load_word(self.body, &environment.status)?;
        self.state.registers.restore(
            self.body,
            environment
                .status
                .unsigned()
                .shr(11)
                .and(7)
                .unsigned()
                .extend::<I32>(),
            &environment.tags,
            registers,
        )?;
        if let Some(opcode) = environment.opcode {
            self.state
                .metadata
                .define(self.body, cpu_location!(x87.opcode), opcode)?;
        }
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.instruction_offset),
            environment.instruction.offset,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.instruction_selector),
            environment.instruction.selector,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.data_offset),
            environment.data.offset,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.data_selector),
            environment.data.selector,
        )?;
        self.state
            .status
            .update_pending_exception(self.body, &mut self.state.control)
    }

    pub(crate) fn stack_bits(&mut self) -> Result<[ExtendedBits; 8], BuildError> {
        self.state.registers.stack_bits(self.body)
    }

    pub(crate) fn mask_exceptions(&mut self) -> Result<(), BuildError> {
        // FNSTENV can follow a suppressed push/pop without waiting. Resolving
        // that mapping before clearing ES prevents a later read assuming success.
        self.state.registers.rebase(self.body)?;
        self.state.control.mask_exceptions(self.body)?;
        self.state
            .status
            .update_pending_exception(self.body, &mut self.state.control)
    }
}

//! Architectural environment images, distinct from the host snapshot layout.

use relocade_compiler::{AtLeast, BuildError, MemoryInt, Val, I1, I16, I32};

mod fx;
mod legacy;

use super::X87Access;
use crate::state::access::cpu_location;
use crate::x87::ExtendedBits;

struct Pointer {
    offset: Val<I32>,
    selector: Val<I16>,
}

struct Environment {
    control: Val<I16>,
    status: Val<I16>,
    tags: Val<I16>,
    opcode: Val<I16>,
    instruction: Pointer,
    data: Pointer,
}

impl X87Access<'_, '_> {
    /// Seven words or dwords in Intel's protected- or real-address-mode layout.
    /// Reserved halfwords are stored as ones and ignored when loading.
    pub(crate) fn environment<T: MemoryInt>(
        &mut self,
        protected: &Val<I1>,
    ) -> Result<[Val<T>; 7], BuildError>
    where
        I32: AtLeast<T>,
    {
        let tags = self.state.registers.saved_tag_word(self.body)?;
        Ok(self.capture_environment(tags)?.encode(protected))
    }

    /// FXSAVE retains only occupancy, so its tags need no payload classification.
    pub(crate) fn fx_environment(
        &mut self,
        pointer_mask: &Val<I32>,
    ) -> Result<[Val<I32>; 6], BuildError> {
        let tags = self.state.registers.tag_word(self.body)?;
        Ok(self.capture_environment(tags)?.encode_fx(pointer_mask))
    }

    fn capture_environment(&mut self, tags: Val<I16>) -> Result<Environment, BuildError> {
        Ok(Environment {
            control: self.control_word()?,
            status: self.status_word()?,
            tags,
            opcode: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.opcode))?,
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
        protected: &Val<I1>,
        words: &[Val<T>; 7],
        registers: Option<&[ExtendedBits; 8]>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let previous_opcode = self
            .state
            .metadata
            .read(self.body, cpu_location!(x87.opcode))?;
        self.restore_environment(
            Environment::decode(words, protected, previous_opcode),
            registers,
        )
    }

    pub(crate) fn restore_fx(
        &mut self,
        pointer_mask: &Val<I32>,
        words: &[Val<I32>; 6],
        registers: &[ExtendedBits; 8],
    ) -> Result<(), BuildError> {
        self.restore_environment(Environment::decode_fx(words, pointer_mask), Some(registers))
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
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.opcode), environment.opcode)?;
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

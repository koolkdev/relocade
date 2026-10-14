//! Architectural environment images, distinct from the host snapshot layout.

use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I32};

mod image;

use super::X87Access;
use crate::state::access::cpu_location;
use image::{Environment, Pointer};

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
        let environment = Environment {
            control: self.control_word()?,
            status: self.status_word()?,
            tags: self.state.registers.saved_tag_word(self.body)?,
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
        };
        Ok(environment.encode(mode))
    }

    pub(crate) fn load_environment<T: MemoryInt>(
        &mut self,
        mode: EnvironmentMode,
        words: &[Val<T>; 7],
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let environment = Environment::decode(words, mode);
        self.state
            .control
            .load_word(self.body, environment.control)?;
        self.state
            .status
            .load_word(self.body, &environment.status)?;
        self.state.registers.load_environment(
            self.body,
            environment
                .status
                .unsigned()
                .shr(11)
                .and(7)
                .unsigned()
                .extend::<I32>(),
            &environment.tags,
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

//! Architectural environment images, distinct from the host snapshot layout.

use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I16, I32};

use super::X87Access;
use crate::state::access::cpu_location;

#[derive(Clone, Copy)]
pub(crate) enum EnvironmentMode {
    Protected,
    Real,
}

struct Environment {
    control: Val<I16>,
    status: Val<I16>,
    tags: Val<I16>,
    opcode: Val<I16>,
    instruction_offset: Val<I32>,
    instruction_selector: Val<I16>,
    data_offset: Val<I32>,
    data_selector: Val<I16>,
}

impl Environment {
    fn encode<T: MemoryInt>(&self, mode: EnvironmentMode) -> [Val<T>; 7]
    where
        I32: AtLeast<T>,
    {
        let wide = T::BYTES == 4;
        let reserved = if wide { 0xffff_0000_u32 } else { 0 };
        let mut words = [
            self.control.unsigned().extend::<I32>().or(reserved),
            self.status.unsigned().extend::<I32>().or(reserved),
            self.tags.unsigned().extend::<I32>().or(reserved),
            self.instruction_offset.clone(),
            self.instruction_selector.unsigned().extend::<I32>(),
            self.data_offset.clone(),
            self.data_selector.unsigned().extend::<I32>().or(reserved),
        ];
        match mode {
            EnvironmentMode::Protected if wide => {
                words[4] = words[4].or(self.opcode.and(0x7ff).unsigned().extend::<I32>().shl(16));
            }
            EnvironmentMode::Protected => {}
            EnvironmentMode::Real => {
                let instruction = self
                    .instruction_offset
                    .add(self.instruction_selector.unsigned().extend::<I32>().shl(4));
                let data = self
                    .data_offset
                    .add(self.data_selector.unsigned().extend::<I32>().shl(4));
                words[3] = instruction.and(0xffff).or(reserved);
                words[4] = instruction.unsigned().shr(4).and(0x0fff_f000).or(self
                    .opcode
                    .and(0x7ff)
                    .unsigned()
                    .extend::<I32>());
                words[5] = data.and(0xffff).or(reserved);
                words[6] = data.unsigned().shr(4).and(0x0fff_f000);
            }
        }
        words.map(|word| word.truncate::<T>())
    }

    fn decode<T: MemoryInt>(
        words: &[Val<T>; 7],
        mode: EnvironmentMode,
        previous_opcode: Val<I16>,
    ) -> Self
    where
        I32: AtLeast<T>,
    {
        let words = words.clone().map(|word| word.unsigned().extend::<I32>());
        let mut environment = Self {
            control: words[0].truncate::<I16>(),
            status: words[1].truncate::<I16>(),
            tags: words[2].truncate::<I16>(),
            opcode: previous_opcode,
            instruction_offset: words[3].clone(),
            instruction_selector: words[4].truncate::<I16>(),
            data_offset: words[5].clone(),
            data_selector: words[6].truncate::<I16>(),
        };
        match mode {
            EnvironmentMode::Protected if T::BYTES == 4 => {
                environment.opcode = words[4].unsigned().shr(16).and(0x7ff).truncate::<I16>();
            }
            EnvironmentMode::Protected => {}
            EnvironmentMode::Real => {
                // The image has linear pointers, not their original segment:offset
                // decomposition. A zero selector preserves them on the next save.
                environment.instruction_offset =
                    words[3].and(0xffff).or(words[4].and(0x0fff_f000).shl(4));
                environment.data_offset = words[5].and(0xffff).or(words[6].and(0x0fff_f000).shl(4));
                environment.instruction_selector = 0.into();
                environment.data_selector = 0.into();
                environment.opcode = words[4].and(0x7ff).truncate::<I16>();
            }
        }
        environment
    }
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
            tags: self.state.registers.tag_word(self.body)?,
            opcode: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.opcode))?,
            instruction_offset: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.instruction_offset))?,
            instruction_selector: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.instruction_selector))?,
            data_offset: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.data_offset))?,
            data_selector: self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.data_selector))?,
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
        // The protected 14-byte image has no opcode field.
        let previous_opcode = match (mode, T::BYTES) {
            (EnvironmentMode::Protected, 2) => self
                .state
                .metadata
                .read(self.body, cpu_location!(x87.opcode))?,
            _ => 0.into(),
        };
        let environment = Environment::decode(words, mode, previous_opcode);
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
        self.state
            .metadata
            .define(self.body, cpu_location!(x87.opcode), environment.opcode)?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.instruction_offset),
            environment.instruction_offset,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.instruction_selector),
            environment.instruction_selector,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.data_offset),
            environment.data_offset,
        )?;
        self.state.metadata.define(
            self.body,
            cpu_location!(x87.data_selector),
            environment.data_selector,
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

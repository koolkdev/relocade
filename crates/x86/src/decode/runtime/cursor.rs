mod read;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I16, I32, I8};

use crate::{
    instruction::{
        FieldWidth, OpcodeMap, OperandSize, SizedForm, EXTENDED_OPCODE_ESCAPE,
        MAX_INSTRUCTION_BYTES,
    },
    memory::Memory,
    state::exit,
};

// One primary opcode and the widest scalar field fit this shared fetch window.
// Longer forms retain the same proof and check fields beyond its extent.
pub(super) const DIRECT_FETCH_BYTES: u32 = 1 + FieldWidth::Dword.bytes();

/// A proven window permits direct reads while every possible cursor position
/// fits its extent. Conditional fields join their final positions while keeping
/// a conservative upper bound. Reads beyond the proof use checked fetch in the
/// wrapping instruction address space.
#[derive(Clone)]
pub(super) struct RuntimeCursor<'memory> {
    memory: &'memory Memory,
    instruction_eip: Val<I32>,
    offset: Val<I32>,
    maximum_offset: u32,
    operand_size: OperandSize,
    opcode_map: OpcodeMap,
    window: Option<Window>,
}

#[derive(Clone)]
struct Window {
    physical_start: Val<I32>,
    fixed_offset: Option<u32>,
    bytes: u32,
}

impl Window {
    fn covers(&self, maximum_offset: u32, bytes: u32) -> bool {
        u64::from(maximum_offset) + u64::from(bytes) <= u64::from(self.bytes)
    }
}

impl<'memory> RuntimeCursor<'memory> {
    pub(super) fn new(
        body: &FunctionBuilder<'_>,
        memory: &'memory Memory,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
        consumed: u32,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            memory,
            instruction_eip: instruction_eip.clone(),
            offset: body.value(consumed)?,
            maximum_offset: consumed,
            operand_size: OperandSize::Dword,
            opcode_map: OpcodeMap::Primary,
            window: physical_start.map(|physical_start| Window {
                physical_start: physical_start.clone(),
                fixed_offset: Some(consumed),
                bytes: DIRECT_FETCH_BYTES,
            }),
        })
    }

    /// Resumes checked reads at the instruction's total consumed byte count.
    /// Prefixes and opcode escapes never start a new instruction-length budget.
    pub(super) fn resume(
        memory: &'memory Memory,
        instruction_eip: &Val<I32>,
        consumed: &Val<I32>,
        operand_size: OperandSize,
    ) -> Self {
        Self {
            memory,
            instruction_eip: instruction_eip.clone(),
            offset: consumed.clone(),
            maximum_offset: MAX_INSTRUCTION_BYTES,
            operand_size,
            opcode_map: OpcodeMap::Primary,
            window: None,
        }
    }

    pub(super) fn select_word_operands(&mut self) {
        self.operand_size = OperandSize::Word;
    }
    pub(super) fn enter_extended_map(&mut self) {
        self.opcode_map = OpcodeMap::Extended;
    }
    pub(super) fn opcode_map(&self) -> OpcodeMap {
        self.opcode_map
    }

    pub(super) fn return_unsupported(
        &self,
        body: FunctionBuilder<'_>,
        selector: &Val<I8>,
    ) -> Result<(), BuildError> {
        let opcode = match self.opcode_map {
            OpcodeMap::Primary => selector.clone(),
            OpcodeMap::Extended => body.value::<I8>(u32::from(EXTENDED_OPCODE_ESCAPE))?,
        };
        body.return_(exit::unsupported(&self.instruction_eip, &opcode))
    }

    pub(super) fn operand_size(&self) -> OperandSize {
        self.operand_size
    }

    pub(super) fn physical_start(&self) -> Option<&Val<I32>> {
        self.window.as_ref().map(|window| &window.physical_start)
    }

    pub(super) fn consumed(&self) -> &Val<I32> {
        &self.offset
    }

    pub(super) fn instruction_eip(&self) -> &Val<I32> {
        &self.instruction_eip
    }

    pub(super) fn next_eip(&self) -> Val<I32> {
        self.instruction_eip.add(&self.offset)
    }

    fn advance(&mut self, bytes: u32) {
        self.offset = self.offset.add(bytes);
        self.maximum_offset = self.maximum_offset.saturating_add(bytes);
        if let Some(window) = &mut self.window {
            if let Some(offset) = &mut window.fixed_offset {
                *offset += bytes;
            }
        }
    }

    fn mark_conditional_offset(&mut self) {
        if let Some(window) = &mut self.window {
            window.fixed_offset = None;
        }
    }

    pub(super) fn immediate(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        form: &SizedForm,
    ) -> Result<Val<I32>, BuildError> {
        if form.sign_extends_immediate() {
            return Ok(self.byte(body)?.signed().extend::<I32>());
        }
        match form.immediate_width() {
            FieldWidth::Byte => Ok(self.byte(body)?.unsigned().extend::<I32>()),
            FieldWidth::Word => Ok(self.read::<I16>(body)?.unsigned().extend::<I32>()),
            FieldWidth::Dword => self.dword(body),
        }
    }

    pub(super) fn displacement(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        mode: &Val<I8>,
        no_base: &Val<I1>,
    ) -> Result<Val<I32>, BuildError> {
        let has_dword_displacement = mode.eq(2).or(no_base);
        let has_byte_displacement = mode.eq(1);
        let (value, next_offset) = body.if_value::<(I32, I32)>(
            &has_dword_displacement,
            |mut dword_body| {
                let mut cursor = self.clone();
                let value = cursor.dword(&mut dword_body)?;
                dword_body.yield_((value, cursor.offset))
            },
            |mut short_displacement_body| {
                let result = short_displacement_body.if_value::<(I32, I32)>(
                    &has_byte_displacement,
                    |mut byte_body| {
                        let mut cursor = self.clone();
                        let value = cursor.byte(&mut byte_body)?;
                        byte_body.yield_((value.signed().extend::<I32>(), cursor.offset))
                    },
                    |no_displacement_body| no_displacement_body.yield_((0, &self.offset)),
                )?;
                short_displacement_body.yield_(result)
            },
        )?;
        self.mark_conditional_offset();
        self.maximum_offset += 4;
        self.offset = next_offset;
        Ok(value)
    }
}

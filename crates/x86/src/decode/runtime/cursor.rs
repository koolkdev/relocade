mod read;

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I16, I32, I8};

use crate::{
    instruction::{
        DecodedFields, Encoding, Location, OperandSize, OperandWidth, ResolvedForm,
        MAX_INSTRUCTION_BYTES, MOV_OPERAND_IMMEDIATE,
    },
    memory::Memory,
    register::RegisterCode,
};

/// A proven window uses fixed displacements; a checked cursor advances in the
/// wrapping instruction address space. A read beyond the proven extent uses
/// checked fetch. Conditional fields discard the window and advance the parent
/// cursor by their selected size, never by retaining a child-scoped position.
#[derive(Clone)]
pub(super) struct RuntimeCursor {
    memory: Memory,
    instruction_eip: Val<I32>,
    offset: Val<I32>,
    maximum_offset: u32,
    operand_size: OperandSize,
    window: Option<Window>,
}

#[derive(Clone)]
struct Window {
    physical_start: Val<I32>,
    consumed: u32,
    bytes: u32,
}

impl Window {
    fn covers(&self, bytes: u32) -> bool {
        u64::from(self.consumed) + u64::from(bytes) <= u64::from(self.bytes)
    }
}

impl RuntimeCursor {
    pub(super) fn new(
        body: &FunctionBuilder<'_>,
        memory: Memory,
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
            window: physical_start.map(|physical_start| Window {
                physical_start: physical_start.clone(),
                consumed,
                bytes: MOV_OPERAND_IMMEDIATE
                    .resolve(OperandSize::Dword)
                    .minimum_length(),
            }),
        })
    }

    /// Resumes after an operand-size prefix, retaining the instruction's total
    /// byte count rather than starting a new cursor at the opcode.
    pub(super) fn after_prefix(
        memory: Memory,
        instruction_eip: &Val<I32>,
        consumed: &Val<I32>,
    ) -> Self {
        Self {
            memory,
            instruction_eip: instruction_eip.clone(),
            offset: consumed.clone(),
            maximum_offset: MAX_INSTRUCTION_BYTES,
            operand_size: OperandSize::Word,
            window: None,
        }
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
            window.consumed += bytes;
        }
    }

    pub(super) fn immediate(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        width: OperandWidth,
    ) -> Result<Val<I32>, BuildError> {
        match width {
            OperandWidth::Byte => Ok(self.byte(body)?.unsigned().extend::<I32>()),
            OperandWidth::Word => Ok(self.read::<I16>(body)?.unsigned().extend::<I32>()),
            OperandWidth::Dword => self.dword(body),
        }
    }

    pub(super) fn modrm_fields(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        form: &ResolvedForm,
        modrm: &Val<I8>,
        rm: Location<Val<I32>>,
    ) -> Result<DecodedFields<Val<I32>>, BuildError> {
        Ok(match form.encoding {
            Encoding::RegisterRm { .. } => DecodedFields::RegisterRm {
                register: RegisterCode::indexed(modrm.unsigned().shr(3).unsigned().extend::<I32>()),
                rm,
            },
            Encoding::RmImmediate { .. } => DecodedFields::RmImmediate {
                rm,
                immediate: self.immediate(body, form.width)?,
            },
            _ => unreachable!("the selected form has a ModRM field"),
        })
    }

    pub(super) fn optional_byte(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        present: &Val<I1>,
    ) -> Result<Val<I8>, BuildError> {
        self.window = None;
        let mut conditional_cursor = self.clone();
        let value = body.if_value::<I8>(
            present,
            |mut field_body| {
                let value = conditional_cursor.byte(&mut field_body)?;
                field_body.yield_(value)
            },
            |absent_field_body| absent_field_body.yield_(0),
        )?;
        self.offset = self.offset.add(present.unsigned().extend::<I32>());
        self.maximum_offset += 1;
        Ok(value)
    }

    pub(super) fn displacement(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        mode: &Val<I8>,
        no_base: &Val<I1>,
    ) -> Result<Val<I32>, BuildError> {
        self.window = None;
        let has_dword_displacement = mode.eq(2).or(no_base);
        let has_byte_displacement = mode.eq(1);
        let value = body.if_value::<I32>(
            &has_dword_displacement,
            |mut dword_body| {
                let value = self.clone().dword(&mut dword_body)?;
                dword_body.yield_(value)
            },
            |mut short_displacement_body| {
                let value = short_displacement_body.if_value::<I32>(
                    &has_byte_displacement,
                    |mut byte_body| {
                        let value = self.clone().byte(&mut byte_body)?;
                        byte_body.yield_(value.signed().extend::<I32>())
                    },
                    |no_displacement_body| no_displacement_body.yield_(0),
                )?;
                short_displacement_body.yield_(value)
            },
        )?;
        self.maximum_offset += 4;
        self.offset = self
            .offset
            .add(has_dword_displacement.unsigned().extend::<I32>().shl(2))
            .add(has_byte_displacement.unsigned().extend::<I32>());
        Ok(value)
    }
}

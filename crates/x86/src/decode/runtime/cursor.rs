mod read;

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I8};

use crate::instruction::{
    DecodedFields, FieldWidth, ImmediateWidth, ResolvedForm, MAX_INSTRUCTION_BYTES,
};

use super::fetch::InstructionFetch;

// One primary opcode and the widest scalar field fit this shared fetch window.
// Longer forms retain the same proof and check fields beyond its extent.
pub(super) const DIRECT_FETCH_BYTES: u32 = 1 + FieldWidth::Dword.bytes();

/// A proven window permits direct reads while every possible cursor position
/// fits its extent. Conditional fields join their final positions while keeping
/// a conservative upper bound. Reads beyond the proof use checked fetch in the
/// wrapping instruction address space.
#[derive(Clone)]
pub(super) struct RuntimeCursor<'memory> {
    pub(super) fetch: InstructionFetch<'memory>,
    instruction_eip: Val<I32>,
    offset: Val<I32>,
    maximum_offset: u32,
    fixed_offset: Option<u32>,
    window: Option<Window>,
}

#[derive(Clone)]
struct Window {
    physical_start: Val<I32>,
    bytes: u32,
}

impl Window {
    fn covers(&self, maximum_offset: u32, bytes: u32) -> bool {
        u64::from(maximum_offset) + u64::from(bytes) <= u64::from(self.bytes)
    }
}

impl<'memory> RuntimeCursor<'memory> {
    pub(super) fn new(
        body: &BlockBuilder<'_>,
        fetch: InstructionFetch<'memory>,
        instruction_eip: &Val<I32>,
        physical_start: Option<&Val<I32>>,
        consumed: u32,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            fetch,
            instruction_eip: instruction_eip.clone(),
            offset: body.value(consumed)?,
            maximum_offset: consumed,
            fixed_offset: Some(consumed),
            window: physical_start.map(|physical_start| Window {
                physical_start: physical_start.clone(),
                bytes: DIRECT_FETCH_BYTES,
            }),
        })
    }

    /// Resumes checked reads at the instruction's total consumed byte count.
    /// Prefixes and opcode escapes never start a new instruction-length budget.
    pub(super) fn resume(
        fetch: InstructionFetch<'memory>,
        instruction_eip: &Val<I32>,
        consumed: &Val<I32>,
    ) -> Self {
        Self {
            fetch,
            instruction_eip: instruction_eip.clone(),
            offset: consumed.clone(),
            maximum_offset: MAX_INSTRUCTION_BYTES,
            fixed_offset: None,
            window: None,
        }
    }

    pub(super) fn fixed_offset(&self) -> Option<u32> {
        self.fixed_offset
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
        if let Some(offset) = &mut self.fixed_offset {
            *offset += bytes;
        }
    }

    fn after_variable_fields(&self, consumed: Val<I32>, maximum_bytes: u32) -> Self {
        let mut cursor = self.clone();
        cursor.fixed_offset = None;
        cursor.maximum_offset += maximum_bytes;
        cursor.offset = consumed;
        cursor
    }

    /// Reads and advances only when present; an absent byte has value zero.
    pub(super) fn byte_if(
        &mut self,
        body: &mut BlockBuilder<'_>,
        present: &Val<I1>,
    ) -> Result<Val<I8>, BuildError> {
        if let Some(present) = body.constant_bits(present)? {
            return if present != 0 {
                self.byte(body)
            } else {
                Ok(0.into())
            };
        }
        let (value, consumed) = body.if_value::<(I8, I32)>(
            present,
            |mut body| {
                let mut cursor = self.clone();
                let value = cursor.byte(&mut body)?;
                body.yield_((value, cursor.offset))
            },
            |body| body.yield_((0, &self.offset)),
        )?;
        *self = self.after_variable_fields(consumed, 1);
        Ok(value)
    }

    /// An absolute address has the effective address width, independently of data width.
    pub(super) fn address_offset(
        &mut self,
        body: &mut BlockBuilder<'_>,
        dword: &Val<I1>,
    ) -> Result<Val<I32>, BuildError> {
        if let Some(dword) = body.constant_bits(dword)? {
            return self.integer(
                body,
                if dword != 0 {
                    FieldWidth::Dword
                } else {
                    FieldWidth::Word
                },
            );
        }
        let read = |mut body: BlockBuilder<'_>, width| {
            let mut cursor = self.clone();
            let value = cursor.integer(&mut body, width)?;
            body.yield_((value, cursor.offset))
        };
        let (value, consumed) = body.if_value::<(I32, I32)>(
            dword,
            |body| read(body, FieldWidth::Dword),
            |body| read(body, FieldWidth::Word),
        )?;
        *self = self.after_variable_fields(consumed, FieldWidth::Dword.bytes());
        Ok(value)
    }

    pub(super) fn read_immediates(
        &mut self,
        body: &mut BlockBuilder<'_>,
        form: &ResolvedForm,
        fields: &mut DecodedFields<Val<I32>>,
    ) -> Result<(), BuildError> {
        for (value, width) in fields.immediates.iter_mut().zip(form.encoding().immediates) {
            *value = width
                .map(|width| self.immediate(body, form, width))
                .transpose()?;
        }
        Ok(())
    }

    fn immediate(
        &mut self,
        body: &mut BlockBuilder<'_>,
        form: &ResolvedForm,
        field: ImmediateWidth,
    ) -> Result<Val<I32>, BuildError> {
        if field.is_signed() {
            return Ok(self.byte(body)?.signed().extend::<I32>());
        }
        self.integer(body, form.immediate_width(field))
    }

    pub(super) fn integer(
        &mut self,
        body: &mut BlockBuilder<'_>,
        width: FieldWidth,
    ) -> Result<Val<I32>, BuildError> {
        match width {
            FieldWidth::Byte => Ok(self.byte(body)?.unsigned().extend::<I32>()),
            FieldWidth::Word => Ok(self.read::<I16>(body)?.unsigned().extend::<I32>()),
            FieldWidth::Dword => self.dword(body),
        }
    }

    pub(super) fn displacement(
        &mut self,
        body: &mut BlockBuilder<'_>,
        mode: &Val<I8>,
        no_base: &Val<I1>,
        dword: &Val<I1>,
    ) -> Result<Val<I32>, BuildError> {
        let has_full_displacement = mode.eq(2).or(no_base);
        let has_byte_displacement = mode.eq(1);
        let (value, next_offset) = body.if_value::<(I32, I32)>(
            &has_full_displacement,
            |mut full_body| {
                let mut cursor = self.clone();
                let value = cursor.address_offset(&mut full_body, dword)?;
                full_body.yield_((value, cursor.offset))
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
        let maximum_width = if body.constant_bits(dword)? == Some(0) {
            FieldWidth::Word
        } else {
            FieldWidth::Dword
        };
        *self = self.after_variable_fields(next_offset, maximum_width.bytes());
        Ok(value)
    }
}

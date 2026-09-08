use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I32, I8};

use crate::{
    fetch,
    instruction::{DecodedFields, Encoding, Form, Location, OperandWidth, MOV_DWORD_IMMEDIATE},
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
            window: physical_start.map(|physical_start| Window {
                physical_start: physical_start.clone(),
                consumed,
                bytes: MOV_DWORD_IMMEDIATE.minimum_length(),
            }),
        })
    }

    pub(super) fn instruction_eip(&self) -> &Val<I32> {
        &self.instruction_eip
    }

    pub(super) fn next_eip(&self) -> Val<I32> {
        self.instruction_eip.add(&self.offset)
    }

    pub(super) fn byte(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I8>, BuildError> {
        let value = match &self.window {
            Some(window) if window.covers(1) => {
                self.memory
                    .load(body, &window.physical_start, window.consumed)?
            }
            _ => fetch::byte(body, self.memory, &self.next_eip())?,
        };
        self.advance(1);
        Ok(value)
    }

    pub(super) fn dword(&mut self, body: &mut FunctionBuilder<'_>) -> Result<Val<I32>, BuildError> {
        let value = match &self.window {
            Some(window) if window.covers(4) => {
                self.memory
                    .load(body, &window.physical_start, window.consumed)?
            }
            _ => fetch::dword(body, self.memory, &self.next_eip())?,
        };
        self.advance(4);
        Ok(value)
    }

    fn advance(&mut self, bytes: u32) {
        self.offset = self.offset.add(bytes);
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
            OperandWidth::Dword => self.dword(body),
        }
    }

    pub(super) fn modrm_fields(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        form: &Form,
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
        self.offset = self
            .offset
            .add(has_dword_displacement.unsigned().extend::<I32>().shl(2))
            .add(has_byte_displacement.unsigned().extend::<I32>());
        Ok(value)
    }
}

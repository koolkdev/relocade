//! Transfer responses resolve stack and numerical exceptions before commitment.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I64};

use crate::x87::{BinaryOperand, ConversionResult, ExtendedValue, RoundingMode};

use super::{control::Exception, StackValue, X87State};

/// Load provenance determines operand exceptions independently of stack faults.
/// Extended transfers and exact integer conversions supply a value directly;
/// only narrow real loads classify SNaNs and denormals as operands.
pub(crate) enum LoadSource {
    Value(ExtendedValue),
    Register(StackValue),
    Binary(BinaryOperand),
}

impl X87State {
    /// Resolves a converted store after the destination's complete access guard.
    /// Precision exceptions commit the store and pop even when unmasked.
    pub(crate) fn prepare_store(
        &mut self,
        body: &mut BlockBuilder<'_>,
        pop: bool,
        convert: impl FnOnce(&ExtendedValue, &RoundingMode) -> ConversionResult,
    ) -> Result<StoreResult, BuildError> {
        let source = self.read_stack(body, 0)?;
        let rounding = self.control.rounding(body)?;
        let result = convert(&source.value.or_indefinite(&source.empty), &rounding);
        let invalid = source.empty.or(result.invalid);
        let unmasked_invalid =
            self.status
                .record_exception(body, Exception::Invalid, &invalid, &mut self.control)?;
        self.status.record_stack_fault(body, &source.empty)?;
        let unmasked_overflow = self.status.record_exception(
            body,
            Exception::Overflow,
            &result.overflow,
            &mut self.control,
        )?;
        let unmasked_underflow = self.control.unmasked(body, Exception::Underflow)?;
        let underflow = result.tiny.and(unmasked_underflow.or(&result.inexact));
        let unmasked_underflow = self.status.record_exception(
            body,
            Exception::Underflow,
            &underflow,
            &mut self.control,
        )?;
        let suppressed = unmasked_invalid
            .or(unmasked_overflow)
            .or(unmasked_underflow);
        let enabled = suppressed.eq(false);
        // Unmasked range exceptions suppress new PE and clear C1 for memory
        // destinations. Existing sticky exception flags remain untouched.
        let precision = enabled.and(result.inexact);
        let unmasked_precision = self.status.record_exception(
            body,
            Exception::Precision,
            &precision,
            &mut self.control,
        )?;
        self.status.set_c1(body, enabled.and(result.incremented))?;
        self.status
            .record_pending_exception(body, suppressed.or(unmasked_precision))?;
        if pop {
            self.pop(body, &enabled)?;
        }
        Ok(StoreResult {
            bits: result.bits,
            enabled,
        })
    }

    pub(crate) fn push_available(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        let target = self.slot(body, 7)?;
        Ok(self.registers.tag(body, &target)?.eq(3))
    }

    pub(crate) fn push(
        &mut self,
        body: &mut BlockBuilder<'_>,
        source: LoadSource,
    ) -> Result<(), BuildError> {
        let (value, source_empty, signaling_nan, denormal) = match source {
            LoadSource::Value(value) => (value, false.into(), false.into(), false.into()),
            LoadSource::Register(source) => {
                (source.value, source.empty, false.into(), false.into())
            }
            LoadSource::Binary(source) => (
                source.value,
                false.into(),
                source.signaling_nan,
                source.denormal,
            ),
        };
        let target = self.slot(body, 7)?;
        let full = self.registers.tag(body, &target)?.ne(3);
        let fault = source_empty.or(&full);
        // A missing source takes priority over an occupied push destination.
        let overflow = source_empty.eq(false).and(full);
        // A stack fault suppresses source conversion exceptions even when the
        // invalid-operation exception is masked.
        let invalid = fault.or(signaling_nan);
        let denormal = invalid.eq(false).and(denormal);
        let unmasked_invalid =
            self.status
                .record_exception(body, Exception::Invalid, &invalid, &mut self.control)?;
        let unmasked_denormal = self.status.record_exception(
            body,
            Exception::Denormal,
            &denormal,
            &mut self.control,
        )?;
        self.status.record_stack_fault(body, &fault)?;
        self.status
            .record_pending_exception(body, unmasked_invalid.or(unmasked_denormal))?;
        self.status.set_c1(body, overflow)?;
        // FLD completes a denormal load even with DM clear (Intel Vol. 2,
        // FLD description). Only an unmasked invalid exception suppresses it.
        let enabled = unmasked_invalid.eq(false);
        let value = value.or_indefinite(&fault);
        let tag = value.tag();
        self.registers.write(body, &target, &value, tag, &enabled)?;
        self.registers.advance(-1);
        self.status.set_top(body, target.physical, enabled)
    }
}

/// Destination bits and write permission after resolving x87 exceptions.
pub(crate) struct StoreResult {
    pub(crate) bits: Val<I64>,
    pub(crate) enabled: Val<I1>,
}

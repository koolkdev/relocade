//! Transfer responses resolve stack and numerical exceptions before commitment.

use wasm86_compiler::{BuildError, Val, I1, I64};

use crate::x87::{BinaryOperand, ConversionResult, ExtendedValue, RoundingMode};

use super::{control::Exception, StackValue, X87Access};

/// Load provenance determines operand exceptions independently of stack faults.
/// Extended transfers and exact integer conversions supply a value directly;
/// only narrow real loads classify SNaNs and denormals as operands.
pub(crate) enum LoadSource {
    Value(ExtendedValue),
    Register(StackValue),
    Binary(BinaryOperand),
}

impl X87Access<'_, '_> {
    /// Resolves converted-store exceptions and commits status and the optional pop.
    /// The caller guards the complete destination first, then writes the returned
    /// bits only when enabled. Unmasked precision exceptions still permit both.
    pub(crate) fn resolve_store(
        &mut self,
        pop: bool,
        convert: impl FnOnce(&ExtendedValue, &RoundingMode) -> ConversionResult,
    ) -> Result<StoreResult, BuildError> {
        let source = self.read_stack(0)?;
        let rounding = self.state.control.rounding(self.body)?;
        let result = convert(&source.value.or_indefinite(&source.is_empty()), &rounding);
        let invalid = source.is_empty().or(result.invalid);
        let unmasked_invalid = self.state.status.record_exception(
            self.body,
            Exception::Invalid,
            &invalid,
            &mut self.state.control,
        )?;
        self.state
            .status
            .record_stack_fault(self.body, &source.is_empty())?;
        let unmasked_overflow = self.state.status.record_exception(
            self.body,
            Exception::Overflow,
            &result.overflow,
            &mut self.state.control,
        )?;
        let unmasked_underflow = self
            .state
            .control
            .unmasked(self.body, Exception::Underflow)?;
        let underflow = result.tiny.and(unmasked_underflow.or(&result.inexact));
        let unmasked_underflow = self.state.status.record_exception(
            self.body,
            Exception::Underflow,
            &underflow,
            &mut self.state.control,
        )?;
        let suppressed = unmasked_invalid
            .or(unmasked_overflow)
            .or(unmasked_underflow);
        let enabled = suppressed.eq(false);
        // Unmasked range exceptions suppress new PE and clear C1 for memory
        // destinations. Existing sticky exception flags remain untouched.
        let precision = enabled.and(result.inexact);
        let unmasked_precision = self.state.status.record_exception(
            self.body,
            Exception::Precision,
            &precision,
            &mut self.state.control,
        )?;
        self.state
            .status
            .set_c1(self.body, enabled.and(result.incremented))?;
        self.state
            .status
            .record_pending_exception(self.body, suppressed.or(unmasked_precision))?;
        if pop {
            self.pop(1, &enabled)?;
        }
        Ok(StoreResult {
            bits: result.bits,
            enabled,
        })
    }

    pub(crate) fn push_available(&mut self) -> Result<Val<I1>, BuildError> {
        let target = self.slot(7)?;
        Ok(self.state.registers.tag(self.body, &target)?.eq(3))
    }

    pub(crate) fn push(&mut self, source: LoadSource) -> Result<(), BuildError> {
        let (value, source_empty, signaling_nan, denormal) = match source {
            LoadSource::Value(value) => (value, false.into(), false.into(), false.into()),
            LoadSource::Register(source) => {
                let empty = source.is_empty();
                (source.value, empty, false.into(), false.into())
            }
            LoadSource::Binary(source) => (
                source.loaded_value(),
                false.into(),
                source.signaling_nan,
                source.denormal,
            ),
        };
        let target = self.slot(7)?;
        let full = self.state.registers.tag(self.body, &target)?.ne(3);
        let fault = source_empty.or(&full);
        // A missing source takes priority over an occupied push destination.
        let overflow = source_empty.eq(false).and(full);
        // A stack fault suppresses source conversion exceptions even when the
        // invalid-operation exception is masked.
        let invalid = fault.or(signaling_nan);
        let denormal = invalid.eq(false).and(denormal);
        let unmasked_invalid = self.state.status.record_exception(
            self.body,
            Exception::Invalid,
            &invalid,
            &mut self.state.control,
        )?;
        let unmasked_denormal = self.state.status.record_exception(
            self.body,
            Exception::Denormal,
            &denormal,
            &mut self.state.control,
        )?;
        self.state.status.record_stack_fault(self.body, &fault)?;
        self.state
            .status
            .record_pending_exception(self.body, unmasked_invalid.or(unmasked_denormal))?;
        self.state.status.set_c1(self.body, overflow)?;
        // FLD completes a denormal load even with DM clear (Intel Vol. 2,
        // FLD description). Only an unmasked invalid exception suppresses it.
        let enabled = unmasked_invalid.eq(false);
        let value = value.or_indefinite(&fault);
        let tag = value.tag();
        self.state
            .registers
            .write(self.body, &target, &value, tag, &enabled)?;
        self.state.registers.advance(-1);
        self.state
            .status
            .set_top(self.body, target.physical, enabled)
    }
}

/// Destination bits and write permission after resolving x87 exceptions.
pub(crate) struct StoreResult {
    pub(crate) bits: Val<I64>,
    pub(crate) enabled: Val<I1>,
}

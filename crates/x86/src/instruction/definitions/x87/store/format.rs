//! Destination formats own their conversion and checked memory representation.

use std::marker::PhantomData;

use crate::{
    execution::{ExecutionBuilder, MemoryOperand},
    floating::{BinaryFormat, RoundingMode},
    memory::TransferType,
    x87::{ConversionResult, ExtendedValue, PackedBcd},
};
use relocade_compiler::{AtLeast, BuildError, MemoryInt, Val, I32, I64};

pub(super) trait StoreFormat {
    type Bits;

    fn bytes(&self) -> u32;
    fn convert(
        &self,
        value: &ExtendedValue,
        rounding: &RoundingMode,
    ) -> ConversionResult<Self::Bits>;
    fn write(
        &self,
        operand: &MemoryOperand,
        execution: &mut ExecutionBuilder<'_, '_>,
        bits: &Self::Bits,
    ) -> Result<(), BuildError>;
}

impl StoreFormat for BinaryFormat {
    type Bits = Val<I64>;

    fn bytes(&self) -> u32 {
        BinaryFormat::bytes(*self)
    }

    fn convert(&self, value: &ExtendedValue, rounding: &RoundingMode) -> ConversionResult {
        self.encode(value, rounding)
    }

    fn write(
        &self,
        operand: &MemoryOperand,
        execution: &mut ExecutionBuilder<'_, '_>,
        bits: &Val<I64>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Binary32 => operand.write(execution, 0, bits.truncate::<I32>()),
            Self::Binary64 => operand.write(execution, 0, bits),
        }
    }
}

pub(super) struct SignedInteger<T>(PhantomData<T>);

impl<T> Default for SignedInteger<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: MemoryInt + TransferType> StoreFormat for SignedInteger<T>
where
    I64: AtLeast<T>,
{
    type Bits = Val<T>;

    fn bytes(&self) -> u32 {
        T::BYTES
    }

    fn convert(&self, value: &ExtendedValue, rounding: &RoundingMode) -> ConversionResult<Val<T>> {
        value
            .to_signed_integer::<T>(rounding)
            .map_bits(|bits| bits.truncate::<T>())
    }

    fn write(
        &self,
        operand: &MemoryOperand,
        execution: &mut ExecutionBuilder<'_, '_>,
        bits: &Val<T>,
    ) -> Result<(), BuildError> {
        operand.write(execution, 0, bits)
    }
}

pub(super) struct PackedDecimal;

impl StoreFormat for PackedDecimal {
    type Bits = PackedBcd;

    fn bytes(&self) -> u32 {
        10
    }

    fn convert(
        &self,
        value: &ExtendedValue,
        rounding: &RoundingMode,
    ) -> ConversionResult<PackedBcd> {
        value.to_packed_bcd(rounding)
    }

    fn write(
        &self,
        operand: &MemoryOperand,
        execution: &mut ExecutionBuilder<'_, '_>,
        bits: &PackedBcd,
    ) -> Result<(), BuildError> {
        operand.write(execution, 0, &bits.low)?;
        operand.write(execution, 8, &bits.high)
    }
}

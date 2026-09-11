mod argument;
mod construction;
mod source;

pub use argument::Argument;

use std::marker::PhantomData;

pub(crate) use source::UnboundExpression;
use source::ValueSource;

use crate::{
    integer::{self, BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    AtLeast, IntType, I1, I32, I64,
};

/// A literal, unbound calculation or function-body integer expression, checked by Rust.
///
/// Native literals use ordinary conversions, such as `Val::<I1>::from(false)` or
/// `Val::<I32>::from(7)`. Literals and calculations containing only literals can
/// be used in any body. A calculation without a constant result stays unbound until
/// admitted to a body. Once an operation uses a body expression, its result
/// belongs to that body, including when it folds to a constant. Folding also
/// preserves the branch visibility required by every original operand.
/// Signed i32 inputs sign-extend and unsigned u32 inputs zero-extend; both keep
/// only the logical low bits in narrower types. A u64 input requires I64.
///
/// Operations construct values immediately. Cloning an expression shares its
/// storage. Construction errors are reported when a value is checked, stored,
/// yielded, returned or passed to a call. Completing or dropping the outer builder
/// closes expression construction in that body. Values depending on a child read,
/// call or join can only be consumed in that child or its descendants.
///
/// A boolean input requires I1:
/// ```compile_fail
/// use wasm86_compiler::{Val, I8};
/// let value = Val::<I8>::from(false);
/// ```
#[derive(Clone)]
pub struct Val<T: IntType> {
    source: ValueSource,
    ty: PhantomData<T>,
}

impl<T: IntType> From<&Val<T>> for Val<T> {
    fn from(value: &Val<T>) -> Self {
        value.clone()
    }
}

impl<T: IntType> From<i32> for Val<T> {
    fn from(value: i32) -> Self {
        Self::literal(value as i64 as u64)
    }
}

impl<T: IntType> From<u32> for Val<T> {
    fn from(value: u32) -> Self {
        Self::literal(u64::from(value))
    }
}

impl From<u64> for Val<I64> {
    fn from(value: u64) -> Self {
        Self::literal(value)
    }
}

impl From<bool> for Val<I1> {
    fn from(value: bool) -> Self {
        Self::literal(u64::from(value))
    }
}

impl<T: IntType> Val<T> {
    /// Returns whether values share a successful representation. Two literals
    /// share their normalized bits; body expressions share their body and node.
    /// Clones of an unbound calculation share its construction recipe.
    /// A literal and a body expression have distinct identities, even when the
    /// expression is constant. This does not compare arbitrary runtime values.
    /// Completing a body preserves identity; consuming its expressions still
    /// checks body ownership and branch visibility.
    pub fn same_expression(&self, other: &Self) -> bool {
        self.source.same_expression(&other.source)
    }

    /// Passes this value in a call argument list, retaining its logical type and
    /// its body ownership and branch visibility. A typed literal keeps its type too.
    pub fn argument(&self) -> Argument {
        self.into()
    }

    /// Adds an integer value or literal of the same type, wrapping on overflow.
    ///
    /// Different integer types cannot be mixed, even when both use Wasm i32:
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I1, I8};
    /// fn add(left: &Val<I1>, right: &Val<I8>) {
    ///     let result = left.add(right);
    /// }
    /// ```
    /// A 64-bit literal requires a 64-bit receiver:
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I32};
    /// fn add(value: &Val<I32>) {
    ///     let result = value.add(1_u64);
    /// }
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn add(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::Add, other)
    }

    /// Subtracts an integer value or literal of the same type, wrapping on underflow.
    /// Narrow results retain their logical low bits, just like addition.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::Sub, other)
    }

    /// Multiplies by an integer value or literal of the same type, wrapping on overflow.
    /// Narrow results retain their logical low bits, just like addition.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::Mul, other)
    }

    /// Keeps bits set in both operands.
    pub fn and(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::And, other)
    }

    /// Sets bits present in either operand.
    pub fn or(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::Or, other)
    }

    /// Keeps bits set in exactly one operand.
    pub fn xor(&self, other: impl Into<Val<T>>) -> Self {
        self.binary(BinaryOp::Xor, other)
    }

    /// Counts set bits in the logical value, ignoring upper carrier bits.
    /// The count retains the receiver's type and always fits in it.
    pub fn popcnt(&self) -> Self {
        self.bit_count(BitCountOp::Ones)
    }

    /// Counts leading zeros in the logical value, ignoring upper carrier bits.
    /// Zero returns the logical width. The count retains the receiver's type.
    pub fn clz(&self) -> Self {
        self.bit_count(BitCountOp::LeadingZeros)
    }

    /// Counts trailing zeros in the logical value, ignoring upper carrier bits.
    /// Zero returns the logical width. The count retains the receiver's type.
    pub fn ctz(&self) -> Self {
        self.bit_count(BitCountOp::TrailingZeros)
    }

    /// Shifts left, retaining the logical type's low bits.
    /// Counts are modulo 32 for I1/I8/I16/I32, and modulo 64 for I64.
    /// In particular, an I8 shift by 8 produces zero; a shift by 32 is identity.
    /// A computed count has type I32, including when shifting an I64 value.
    #[allow(clippy::should_implement_trait)]
    pub fn shl(&self, count: impl Into<Val<I32>>) -> Self {
        self.shift(ShiftOp::Left, count)
    }

    /// Rotates the logical bits left. The I32 count wraps modulo the logical width.
    pub fn rotl(&self, count: impl Into<Val<I32>>) -> Self {
        self.rotate(RotateOp::Left, count)
    }

    /// Rotates the logical bits right. The I32 count wraps modulo the logical width.
    pub fn rotr(&self, count: impl Into<Val<I32>>) -> Self {
        self.rotate(RotateOp::Right, count)
    }

    /// Tests whether the operands' logical low bits are equal.
    pub fn eq(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.compare(CompareOp::Eq, other)
    }

    /// Tests whether the operands' logical low bits differ.
    pub fn ne(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.compare(CompareOp::Ne, other)
    }

    /// Reads this integer's logical bits as an unsigned value.
    /// Creating the view does not construct an expression.
    pub fn unsigned(&self) -> Unsigned<'_, T> {
        Unsigned(self)
    }

    /// Reads the logical sign bit as a two's-complement sign.
    /// Creating the view does not construct an expression.
    pub fn signed(&self) -> Signed<'_, T> {
        Signed(self)
    }

    /// Retains the destination type's low bits. The destination cannot be wider.
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I8, I32};
    /// fn narrow(value: &Val<I8>) {
    ///     let result = value.truncate::<I32>();
    /// }
    /// ```
    pub fn truncate<To: IntType>(&self) -> Val<To>
    where
        T: AtLeast<To>,
    {
        self.convert()
    }

    fn binary(&self, operator: BinaryOp, other: impl Into<Val<T>>) -> Self {
        let other: Self = other.into();
        self.combine(
            &other,
            |left, right| integer::binary(T::TYPE, operator, left, right),
            move |arena, left, right| arena.binary(operator, left, right),
        )
    }

    fn bit_count(&self, operator: BitCountOp) -> Self {
        self.map(
            |bits| integer::bit_count(T::TYPE, operator, bits),
            move |arena, input| arena.bit_count(operator, input),
        )
    }

    fn compare(&self, operator: CompareOp, other: impl Into<Val<T>>) -> Val<I1> {
        let other: Self = other.into();
        self.combine(
            &other,
            |left, right| Some(u64::from(integer::compare(T::TYPE, operator, left, right))),
            move |arena, left, right| arena.compare(operator, left, right),
        )
    }

    fn shift(&self, operator: ShiftOp, count: impl Into<Val<I32>>) -> Self {
        let count: Val<I32> = count.into();
        self.combine(
            &count,
            |value, count| Some(integer::shift(T::TYPE, operator, value, count as u32)),
            move |arena, value, count| arena.shift(operator, value, count),
        )
    }

    fn rotate(&self, operator: RotateOp, count: impl Into<Val<I32>>) -> Self {
        let count: Val<I32> = count.into();
        self.combine(
            &count,
            |value, count| Some(integer::rotate(T::TYPE, operator, value, count as u32)),
            move |arena, value, count| arena.rotate(operator, value, count),
        )
    }

    fn convert<To: IntType>(&self) -> Val<To> {
        self.map(|bits| bits, |arena, input| arena.convert(input, To::TYPE))
    }
}

/// An unsigned interpretation of a borrowed integer value.
pub struct Unsigned<'a, T: IntType>(&'a Val<T>);

impl<T: IntType> Unsigned<'_, T> {
    /// Divides unsigned logical integers. The divisor must be nonzero.
    /// Like other value calculations, division is placed at its uses.
    #[allow(clippy::should_implement_trait)]
    pub fn div(&self, other: impl Into<Val<T>>) -> Val<T> {
        self.0.binary(BinaryOp::DivUnsigned, other)
    }

    /// Computes the unsigned logical remainder. The divisor must be nonzero.
    #[allow(clippy::should_implement_trait)]
    pub fn rem(&self, other: impl Into<Val<T>>) -> Val<T> {
        self.0.binary(BinaryOp::RemUnsigned, other)
    }

    /// Shifts the logical bits right, filling with zeros.
    /// Counts are modulo 32 for I1/I8/I16/I32, and modulo 64 for I64.
    /// An I8 shift by 8 produces zero, including for values with bit 7 set.
    /// A computed count has type I32, including when shifting an I64 value.
    #[allow(clippy::should_implement_trait)]
    pub fn shr(&self, count: impl Into<Val<I32>>) -> Val<T> {
        self.0.shift(ShiftOp::RightUnsigned, count)
    }

    /// Tests unsigned less-than and returns a logical one-bit value.
    pub fn lt(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.0.compare(CompareOp::LtUnsigned, other)
    }

    /// Tests unsigned greater-than-or-equal and returns a logical one-bit value.
    pub fn ge(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.0.compare(CompareOp::GeUnsigned, other)
    }

    /// Widens the logical value with zero bits. The destination cannot be narrower.
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I8, I32};
    /// fn widen(value: &Val<I32>) {
    ///     let result = value.unsigned().extend::<I8>();
    /// }
    /// ```
    pub fn extend<To: AtLeast<T>>(&self) -> Val<To> {
        self.0.convert()
    }
}

/// A signed interpretation of a borrowed integer value.
pub struct Signed<'a, T: IntType>(&'a Val<T>);

impl<T: IntType> Signed<'_, T> {
    /// Divides signed logical integers, truncating toward zero and retaining the
    /// logical low bits. The divisor must be nonzero; I32 and I64 quotients must
    /// fit their signed type. Like other calculations, division is placed at its uses.
    #[allow(clippy::should_implement_trait)]
    pub fn div(&self, other: impl Into<Val<T>>) -> Val<T> {
        self.0.binary(BinaryOp::DivSigned, other)
    }

    /// Computes signed logical remainder, with the dividend's sign or zero.
    /// The divisor must be nonzero. The signed minimum modulo -1 is zero.
    #[allow(clippy::should_implement_trait)]
    pub fn rem(&self, other: impl Into<Val<T>>) -> Val<T> {
        self.0.binary(BinaryOp::RemSigned, other)
    }

    /// Shifts right, repeating the logical sign bit.
    /// Accepts an I32 count or a literal; counts wrap modulo 64 for I64 and 32 otherwise.
    #[allow(clippy::should_implement_trait)]
    pub fn shr(&self, count: impl Into<Val<I32>>) -> Val<T> {
        self.0.shift(ShiftOp::RightSigned, count)
    }

    /// Tests signed less-than using each operand's logical sign bit.
    /// For I1, true is -1 and false is zero.
    pub fn lt(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.0.compare(CompareOp::LtSigned, other)
    }

    /// Tests signed greater-than-or-equal using each operand's logical sign bit.
    /// Arithmetic wraps before its result is interpreted as signed.
    pub fn ge(&self, other: impl Into<Val<T>>) -> Val<I1> {
        self.0.compare(CompareOp::GeSigned, other)
    }

    /// Widens by repeating the source's logical sign bit. The destination cannot
    /// be narrower. For I1, the bit pattern 1 extends to all ones.
    pub fn extend<To: AtLeast<T>>(&self) -> Val<To> {
        self.0.map(
            |bits| integer::signed_value(T::TYPE, bits) as u64,
            |arena, input| arena.sign_extend(input, To::TYPE),
        )
    }
}

#[cfg(test)]
mod tests;

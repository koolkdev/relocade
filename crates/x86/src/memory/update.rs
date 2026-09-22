//! Naturally aligned native atomics and private-memory unaligned updates.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryInt, Val, I32};

use super::{Access, Intent, Memory};
use crate::alu::OperandUpdate;

fn native_update<T: MemoryInt>(
    body: &mut FunctionBuilder<'_>,
    guest: Mem,
    physical: &Val<I32>,
    update: &OperandUpdate<T>,
) -> Result<Val<T>, BuildError> {
    let access = body.atomic::<T>(guest, physical, 0)?;
    match update {
        OperandUpdate::Add(value) => access.add(value),
        OperandUpdate::AddWithCarry { source, carry } => {
            access.add(source.add(carry.unsigned().extend::<T>()))
        }
        OperandUpdate::Subtract(value) => access.sub(value),
        OperandUpdate::SubtractWithBorrow { source, borrow } => {
            access.sub(source.add(borrow.unsigned().extend::<T>()))
        }
        OperandUpdate::And(value) => access.and(value),
        OperandUpdate::Or(value) => access.or(value),
        OperandUpdate::Xor(value) => access.xor(value),
        OperandUpdate::Exchange(value) => access.exchange(value),
        OperandUpdate::CompareExchange {
            expected,
            replacement,
        } => access.compare_exchange(expected, replacement),
        OperandUpdate::Negate => {
            let initial = access.load()?;
            body.loop_::<T, T>(initial, |mut iteration, labels, expected| {
                let replacement = update.apply(&expected);
                let previous = iteration
                    .atomic::<T>(guest, physical, 0)?
                    .compare_exchange(&expected, replacement)?;
                iteration.branch_if(previous.eq(expected), &labels.exit, &previous)?;
                iteration.branch(&labels.again, previous)
            })
        }
    }
}

impl Memory {
    /// Returns the value observed by this complete read-modify-write operation.
    /// Naturally aligned scalar operands fit one page, so their native path
    /// does not inspect scattered backing. Translation preserves page offsets.
    pub(crate) fn atomic_update<T: MemoryInt>(
        &self,
        body: &mut FunctionBuilder<'_>,
        access: &Access,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        assert!(matches!(access.intent, Intent::Write));
        assert_eq!(
            access.bytes,
            T::BYTES,
            "an atomic update covers its complete checked operand"
        );
        if T::BYTES == 1 {
            return native_update(body, self.guest, &access.physical, update);
        }
        body.if_value::<T>(
            access.linear.and(T::BYTES - 1).eq(0),
            |mut aligned| {
                let previous = native_update(&mut aligned, self.guest, &access.physical, update)?;
                aligned.yield_(previous)
            },
            |mut unaligned| {
                // Guest RAM is private. This complete checked update cannot
                // interleave with another guest; shared RAM needs host coordination.
                let previous = self.read::<T>(&mut unaligned, access, 0)?;
                self.write(&mut unaligned, access, 0, &update.apply(&previous))?;
                unaligned.yield_(previous)
            },
        )
    }
}

#[cfg(test)]
mod tests;

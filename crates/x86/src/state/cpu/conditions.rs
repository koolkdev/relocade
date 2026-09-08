//! Stored condition queries and their shared fallback readers.

use wasm86_compiler::{
    AtLeast, BuildError, Func, FunctionBuilder, MemoryInt, Program, Signature, Type, Val, I1, I16,
    I32, I8,
};

use crate::flags::{logic_flag, ArithmeticKind, ArithmeticSource, Condition};

use super::{super::flags, Cpu};

impl Cpu {
    /// Reads a stored condition without materializing flags. CMP relations stay
    /// in the consumer; other records use the shared readonly reader.
    pub(in crate::state) fn read_condition(
        &self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let Some(compare_dword) = condition.operand_comparison::<I32>() else {
            return self.read_condition_fallback(body, condition);
        };
        let kind = body.load::<I8>(self.memory, flags::KIND_OFFSET)?;
        let is_sub = kind
            .eq(u32::from(flags::encode_kind::<I8>(ArithmeticKind::Sub)))
            .or(kind.eq(u32::from(flags::encode_kind::<I16>(ArithmeticKind::Sub))))
            .or(kind.eq(u32::from(flags::encode_kind::<I32>(ArithmeticKind::Sub))));
        body.if_value::<I1>(
            is_sub,
            |mut arm| {
                let left = arm.load::<I32>(self.memory, flags::LEFT_OFFSET)?;
                let right = arm.load::<I32>(self.memory, flags::RIGHT_OFFSET)?;
                let byte = compare_operands::<I8>(condition, &left, &right);
                let word = compare_operands::<I16>(condition, &left, &right);
                let dword = compare_dword(&left, &right);
                let wider = kind
                    .unsigned()
                    .lt(u32::from(flags::width_code::<I32>()))
                    .select(word, dword);
                let compared = kind
                    .unsigned()
                    .lt(u32::from(flags::width_code::<I16>()))
                    .select(byte, wider);
                arm.yield_(compared)
            },
            |mut fallback| {
                let result = self.read_condition_fallback(&mut fallback, condition)?;
                fallback.yield_(result)
            },
        )
    }

    fn read_condition_fallback(
        &self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let resolver = self.condition_resolver(body.program(), condition)?;
        let value = body.call::<I1>(resolver, &[])?;
        Ok(if condition.is_inverted() {
            value.eq(0)
        } else {
            value
        })
    }

    /// Returns the shared reader for the canonical member of an inverse pair.
    /// State caches that result and inverts it for the opposite condition.
    fn condition_resolver(
        &self,
        program: &mut Program,
        condition: Condition,
    ) -> Result<Func, BuildError> {
        let canonical = condition.canonical();
        let slot = &self.condition_resolvers[flags::condition_index(canonical)];
        if let Some(function) = slot.get() {
            return Ok(function);
        }
        let function = program.function(
            Signature {
                parameters: vec![],
                result: Type::I1,
            },
            |body| self.define_condition_resolver(body, canonical),
        )?;
        slot.set(Some(function));
        Ok(function)
    }

    fn define_condition_resolver(
        &self,
        mut body: FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<(), BuildError> {
        let kind = body.load::<I8>(self.memory, flags::KIND_OFFSET)?;
        body.if_(kind.eq(0), |mut arm| {
            let result = condition.evaluate(|flag| {
                let concrete = arm.load::<I8>(
                    self.memory,
                    flags::CONCRETE_OFFSET + flags::status_index(flag) as u32,
                )?;
                Ok::<_, BuildError>(concrete.truncate::<I1>())
            })?;
            arm.return_(result)
        })?;
        let left = body.load::<I32>(self.memory, flags::LEFT_OFFSET)?;
        let right = body.load::<I32>(self.memory, flags::RIGHT_OFFSET)?;
        // The kind groups records by operand width; dispatch that region before
        // testing its operation so later widths do not scan earlier operations.
        body.if_(
            kind.unsigned().lt(u32::from(flags::width_code::<I16>())),
            |arm| return_condition::<I8>(arm, &kind, &left, &right, condition),
        )?;
        body.if_(
            kind.unsigned().lt(u32::from(flags::width_code::<I32>())),
            |arm| return_condition::<I16>(arm, &kind, &left, &right, condition),
        )?;
        return_condition::<I32>(body, &kind, &left, &right, condition)
    }
}

fn return_condition<T: MemoryInt>(
    mut body: FunctionBuilder<'_>,
    stored_kind: &Val<I8>,
    left: &Val<I32>,
    right: &Val<I32>,
    condition: Condition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let left = left.truncate::<T>();
    let right = right.truncate::<T>();
    for kind in [ArithmeticKind::Sub, ArithmeticKind::Add] {
        let source = ArithmeticSource::new(kind, left.clone(), right.clone());
        body.if_(
            stored_kind.eq(u32::from(flags::encode_kind::<T>(kind))),
            |arm| arm.return_(source.condition(condition)),
        )?;
    }
    let logic_kind = flags::width_code::<T>() | 3;
    body.if_(stored_kind.eq(u32::from(logic_kind)), |arm| {
        let result = condition.evaluate(|flag| Ok::<_, BuildError>(logic_flag(&left, flag)))?;
        arm.return_(result)
    })?;
    // The selected width accepts only its SUB, ADD and logic source tags.
    body.trap()
}

fn compare_operands<T: MemoryInt>(
    condition: Condition,
    left: &Val<I32>,
    right: &Val<I32>,
) -> Val<I1>
where
    I32: AtLeast<T>,
{
    let compare = condition
        .operand_comparison::<T>()
        .expect("the condition has an operand comparison");
    compare(&left.truncate::<T>(), &right.truncate::<T>())
}

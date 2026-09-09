//! Stored condition queries and their shared fallback readers.

use wasm86_compiler::{
    AtLeast, BuildError, Func, FunctionBuilder, MemoryInt, Program, Signature, Type, Val, I1, I16,
    I32, I8,
};

use crate::flags::{ArithmeticKind, ArithmeticSource, Condition, FlagSource};

use super::{super::flags, Cpu};

/// Builds one typed comparison inside an already selected record case.
type RecordQuery = fn(&Cpu, &mut FunctionBuilder<'_>, Condition) -> Result<Val<I1>, BuildError>;

impl Cpu {
    /// Reads a stored condition without materializing flags. Subtraction relations
    /// and logical zero tests stay in the consumer; other records use the shared
    /// readonly reader.
    pub(in crate::state) fn read_condition(
        &self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        if condition.operand_comparison::<I32>().is_none() {
            return self.read_condition_fallback(body, condition);
        }
        let mut queries: Vec<(u32, RecordQuery)> = vec![
            (
                u32::from(flags::encode_kind::<I8>(ArithmeticKind::Sub)),
                read_subtraction::<I8>,
            ),
            (
                u32::from(flags::encode_kind::<I16>(ArithmeticKind::Sub)),
                read_subtraction::<I16>,
            ),
            (
                u32::from(flags::encode_kind::<I32>(ArithmeticKind::Sub)),
                read_subtraction::<I32>,
            ),
        ];
        if condition.logic_result_comparison::<I32>().is_some() {
            queries.extend([
                (
                    u32::from(flags::encode_logic::<I8>()),
                    read_logic::<I8> as RecordQuery,
                ),
                (
                    u32::from(flags::encode_logic::<I16>()),
                    read_logic::<I16> as RecordQuery,
                ),
                (
                    u32::from(flags::encode_logic::<I32>()),
                    read_logic::<I32> as RecordQuery,
                ),
            ]);
        }
        queries.sort_unstable_by_key(|(kind, _)| *kind);
        let kinds: Vec<_> = queries.iter().map(|(kind, _)| *kind).collect();
        let kind = body.load::<I8>(self.memory, flags::KIND_OFFSET)?;
        body.switch_value::<I1, _>(&kind, &kinds, |mut arm, kind| {
            let result = match kind {
                Some(kind) => {
                    let (_, query) = queries
                        .iter()
                        .find(|(key, _)| *key == kind)
                        .expect("the switch selects a declared record query");
                    query(self, &mut arm, condition)?
                }
                None => self.read_condition_fallback(&mut arm, condition)?,
            };
            arm.yield_(result)
        })
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
    body.if_(
        stored_kind.eq(u32::from(flags::encode_logic::<T>())),
        |arm| arm.return_(FlagSource::Logic { result: left }.condition(condition)),
    )?;
    // The selected width accepts only its SUB, ADD and logic source tags.
    body.trap()
}

fn read_subtraction<T: MemoryInt>(
    cpu: &Cpu,
    body: &mut FunctionBuilder<'_>,
    condition: Condition,
) -> Result<Val<I1>, BuildError>
where
    I32: AtLeast<T>,
{
    let compare = condition
        .operand_comparison::<T>()
        .expect("the condition has an operand comparison");
    let left = body.load::<I32>(cpu.memory, flags::LEFT_OFFSET)?;
    let right = body.load::<I32>(cpu.memory, flags::RIGHT_OFFSET)?;
    Ok(compare(&left.truncate::<T>(), &right.truncate::<T>()))
}

fn read_logic<T: MemoryInt>(
    cpu: &Cpu,
    body: &mut FunctionBuilder<'_>,
    condition: Condition,
) -> Result<Val<I1>, BuildError>
where
    I32: AtLeast<T>,
{
    let compare = condition
        .logic_result_comparison::<T>()
        .expect("the condition has a logical result comparison");
    let result = body.load::<I32>(cpu.memory, flags::LEFT_OFFSET)?;
    Ok(compare(&result.truncate::<T>()))
}

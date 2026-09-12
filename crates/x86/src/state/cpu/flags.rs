//! Stored condition and flag-subset queries over CPU records.

use wasm86_compiler::{
    AtLeast, BuildError, Func, FunctionBuilder, MemoryInt, Program, Signature, Type, Val, I1, I16,
    I32, I8,
};

use crate::alu::{AnyStatusSource, ArithmeticOp, StatusSource};
use crate::flags::{Condition, FlagMask, StatusFlag};

use super::{
    super::{
        access::cpu_load,
        flags::{condition_index, record},
    },
    Cpu,
};

/// Builds one typed comparison inside an already selected record case.
type RecordQuery = fn(&Cpu, &mut FunctionBuilder<'_>, Condition) -> Result<Val<I1>, BuildError>;

#[derive(Clone, Copy)]
enum StoredQuery {
    Condition(Condition),
    Flags(FlagMask),
}

impl StoredQuery {
    fn result_types(self) -> Vec<Type> {
        match self {
            Self::Condition(_) => vec![Type::I1],
            Self::Flags(mask) => mask.status_flags().map(|_| Type::I1).collect(),
        }
    }

    fn return_concrete(self, cpu: &Cpu, mut body: FunctionBuilder<'_>) -> Result<(), BuildError> {
        match self {
            Self::Condition(condition) => {
                let result = condition.evaluate(|flag| cpu.read_concrete_flag(&mut body, flag))?;
                body.return_(result)
            }
            Self::Flags(mask) => {
                let results = mask
                    .status_flags()
                    .map(|flag| cpu.read_concrete_flag(&mut body, flag))
                    .collect::<Result<Vec<_>, _>>()?;
                body.return_(results)
            }
        }
    }

    fn return_from_source(
        self,
        body: FunctionBuilder<'_>,
        source: &AnyStatusSource,
    ) -> Result<(), BuildError> {
        match self {
            Self::Condition(condition) => body.return_(source.condition(condition)),
            Self::Flags(mask) => body.return_(
                mask.status_flags()
                    .map(|flag| source.flag(flag))
                    .collect::<Vec<_>>(),
            ),
        }
    }
}

fn call_flags(
    body: &mut FunctionBuilder<'_>,
    resolver: Func,
    count: usize,
) -> Result<Vec<Val<I1>>, BuildError> {
    match count {
        1 => body.call::<[I1; 1]>(resolver, &[]).map(Vec::from),
        2 => body.call::<[I1; 2]>(resolver, &[]).map(Vec::from),
        3 => body.call::<[I1; 3]>(resolver, &[]).map(Vec::from),
        4 => body.call::<[I1; 4]>(resolver, &[]).map(Vec::from),
        5 => body.call::<[I1; 5]>(resolver, &[]).map(Vec::from),
        6 => body.call::<[I1; 6]>(resolver, &[]).map(Vec::from),
        _ => unreachable!("a nonempty flag subset has one to six results"),
    }
}

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
                u32::from(record::encode_kind::<I8>(ArithmeticOp::Subtract)),
                read_subtraction::<I8>,
            ),
            (
                u32::from(record::encode_kind::<I16>(ArithmeticOp::Subtract)),
                read_subtraction::<I16>,
            ),
            (
                u32::from(record::encode_kind::<I32>(ArithmeticOp::Subtract)),
                read_subtraction::<I32>,
            ),
        ];
        if condition.logic_result_comparison::<I32>().is_some() {
            queries.extend([
                (
                    u32::from(record::encode_logic::<I8>()),
                    read_logic::<I8> as RecordQuery,
                ),
                (
                    u32::from(record::encode_logic::<I16>()),
                    read_logic::<I16> as RecordQuery,
                ),
                (
                    u32::from(record::encode_logic::<I32>()),
                    read_logic::<I32> as RecordQuery,
                ),
            ]);
        }
        queries.sort_unstable_by_key(|(kind, _)| *kind);
        let kinds: Vec<_> = queries.iter().map(|(kind, _)| *kind).collect();
        let kind = cpu_load!(body, self.memory, flags.status_source.kind)?;
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

    /// Resolves a subset in one record dispatch; unrequested slots are empty.
    /// Reading never changes CPU storage.
    pub(in crate::state) fn read_flags(
        &self,
        body: &mut FunctionBuilder<'_>,
        mask: FlagMask,
    ) -> Result<[Option<Val<I1>>; 6], BuildError> {
        debug_assert!(FlagMask::STATUS.covers(mask));
        let mut flags = StatusFlag::ALL.map(|_| None);
        if mask.bits() == 0 {
            return Ok(flags);
        }
        let resolver = self.flag_resolver(body.program(), StoredQuery::Flags(mask))?;
        let values = call_flags(body, resolver, mask.status_flags().count())?;
        for (flag, value) in mask.status_flags().zip(values) {
            flags[flag as usize] = Some(value);
        }
        Ok(flags)
    }

    fn read_condition_fallback(
        &self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let resolver = self.flag_resolver(
            body.program(),
            StoredQuery::Condition(condition.canonical()),
        )?;
        let value = body.call::<I1>(resolver, &[])?;
        Ok(if condition.is_inverted() {
            value.eq(0)
        } else {
            value
        })
    }

    /// Shares canonical condition readers and readers for matching flag subsets.
    /// State owns result caching on the path where a reader is called.
    fn flag_resolver(&self, program: &mut Program, query: StoredQuery) -> Result<Func, BuildError> {
        let slot = match query {
            StoredQuery::Condition(condition) => {
                &self.condition_resolvers[condition_index(condition)]
            }
            StoredQuery::Flags(mask) => &self.flag_resolvers[usize::from(mask.bits())],
        };
        if let Some(function) = slot.get() {
            return Ok(function);
        }
        let function = program.function(
            Signature {
                parameters: vec![],
                results: query.result_types(),
            },
            |body| self.define_flag_resolver(body, query),
        )?;
        slot.set(Some(function));
        Ok(function)
    }

    fn define_flag_resolver(
        &self,
        mut body: FunctionBuilder<'_>,
        query: StoredQuery,
    ) -> Result<(), BuildError> {
        let kind = cpu_load!(&mut body, self.memory, flags.status_source.kind)?;
        body.if_(kind.eq(u32::from(record::CONCRETE_KIND)), |arm| {
            query.return_concrete(self, arm)
        })?;
        let left = cpu_load!(&mut body, self.memory, flags.status_source.left)?;
        let right = cpu_load!(&mut body, self.memory, flags.status_source.right)?;
        // The kind groups records by operand width; dispatch that region before
        // testing its operation so later widths do not scan earlier operations.
        body.if_(
            kind.unsigned().lt(u32::from(record::width_code::<I16>())),
            |arm| return_query::<I8>(arm, &kind, &left, &right, query),
        )?;
        body.if_(
            kind.unsigned().lt(u32::from(record::width_code::<I32>())),
            |arm| return_query::<I16>(arm, &kind, &left, &right, query),
        )?;
        return_query::<I32>(body, &kind, &left, &right, query)
    }

    fn read_concrete_flag(
        &self,
        body: &mut FunctionBuilder<'_>,
        flag: StatusFlag,
    ) -> Result<Val<I1>, BuildError> {
        let value = match flag {
            StatusFlag::CF => cpu_load!(body, self.memory, flags.bytes.cf)?,
            StatusFlag::PF => cpu_load!(body, self.memory, flags.bytes.pf)?,
            StatusFlag::AF => cpu_load!(body, self.memory, flags.bytes.af)?,
            StatusFlag::ZF => cpu_load!(body, self.memory, flags.bytes.zf)?,
            StatusFlag::SF => cpu_load!(body, self.memory, flags.bytes.sf)?,
            StatusFlag::OF => cpu_load!(body, self.memory, flags.bytes.of)?,
        };
        Ok(value.truncate::<I1>())
    }
}

fn return_query<T: MemoryInt>(
    mut body: FunctionBuilder<'_>,
    stored_kind: &Val<I8>,
    left: &Val<I32>,
    right: &Val<I32>,
    query: StoredQuery,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = left.truncate::<T>();
    let right = right.truncate::<T>();
    for operation in [ArithmeticOp::Subtract, ArithmeticOp::Add] {
        let source = StatusSource::Arithmetic {
            operation,
            left: left.clone(),
            right: right.clone(),
            result: operation.result(&left, &right),
        }
        .into();
        body.if_(
            stored_kind.eq(u32::from(record::encode_kind::<T>(operation))),
            |arm| query.return_from_source(arm, &source),
        )?;
    }
    body.if_(
        stored_kind.eq(u32::from(record::encode_logic::<T>())),
        |arm| query.return_from_source(arm, &StatusSource::Logic { result: left }.into()),
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
    let left = cpu_load!(body, cpu.memory, flags.status_source.left)?;
    let right = cpu_load!(body, cpu.memory, flags.status_source.right)?;
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
    let result = cpu_load!(body, cpu.memory, flags.status_source.left)?;
    Ok(compare(&result.truncate::<T>()))
}

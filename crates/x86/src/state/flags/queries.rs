//! Condition shortcuts and demand-driven composition of individual status bits.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1};

use crate::alu::flags::{Condition, FlagChange, FlagMask, FlagValues, StatusFlag};
use crate::state::{Cpu, State};

use super::{condition_index, FlagBase};

#[derive(Clone)]
pub(super) struct StoredFlagCache {
    conditions: [Option<Val<I1>>; Condition::CANONICAL.len()],
    auxiliary: Option<Val<I1>>,
}

impl Default for StoredFlagCache {
    fn default() -> Self {
        Self {
            conditions: Condition::CANONICAL.map(|_| None),
            auxiliary: None,
        }
    }
}

impl StoredFlagCache {
    // Single-flag conditions and subset reads share a slot. AF has no condition.
    fn flag_slot(&mut self, flag: StatusFlag) -> &mut Option<Val<I1>> {
        let condition = match flag {
            StatusFlag::CF => Condition::B,
            StatusFlag::PF => Condition::P,
            StatusFlag::AF => return &mut self.auxiliary,
            StatusFlag::ZF => Condition::E,
            StatusFlag::SF => Condition::S,
            StatusFlag::OF => Condition::O,
        };
        &mut self.conditions[condition_index(condition)]
    }
}

impl State<'_> {
    pub(crate) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let needed = condition.flags();
        let partial = self.flags.updates.iter().rposition(|update| {
            matches!(update.values, FlagValues::Partial(_)) && update.writes().intersects(needed)
        });
        let (mut value, following) = if let Some(index) = partial {
            let flags = resolve_flags(
                body,
                self.cpu,
                &mut self.flags.base,
                &self.flags.updates[..=index],
                needed,
            )?;
            let value = condition.evaluate(|flag| {
                Ok::<_, BuildError>(flags[flag as usize].as_ref().unwrap().clone())
            })?;
            (value, &self.flags.updates[index + 1..])
        } else {
            (
                self.flags.base.condition(body, self.cpu, condition)?,
                self.flags.updates.as_slice(),
            )
        };
        // All relevant partial changes are already composed. Newer complete
        // sources retain their subtraction and logical-result shortcuts.
        for update in following {
            if let FlagValues::Complete(source) = &update.values {
                value = update
                    .condition
                    .as_ref()
                    .unwrap()
                    .select(source.condition(condition), value);
            }
        }
        body.value(value)
    }
}

pub(super) fn resolve_flags(
    body: &mut FunctionBuilder<'_>,
    cpu: &Cpu,
    base: &mut FlagBase,
    updates: &[FlagChange],
    needed: FlagMask,
) -> Result<[Option<Val<I1>>; 6], BuildError> {
    // An unconditional definition cuts off the older history of that bit.
    // Discover the remaining backing reads together before constructing values.
    let starts = StatusFlag::ALL.map(|flag| {
        updates
            .iter()
            .rposition(|update| update.condition.is_none() && update.writes().contains(flag))
    });
    let inherited = StatusFlag::ALL.iter().fold(FlagMask::EMPTY, |mask, flag| {
        if needed.contains(*flag) && starts[*flag as usize].is_none() {
            mask.union(FlagMask::of(*flag))
        } else {
            mask
        }
    });
    let mut flags = base.read_flags(body, cpu, inherited)?;
    for flag in StatusFlag::ALL {
        if !needed.contains(flag) {
            continue;
        }
        let start = starts[flag as usize];
        let (mut value, following) = if let Some(index) = start {
            (updates[index].flag(flag), &updates[index + 1..])
        } else {
            (flags[flag as usize].take().unwrap(), updates)
        };
        for update in following {
            if update.writes().contains(flag) {
                let changed = update.flag(flag);
                value = match &update.condition {
                    Some(condition) => condition.select(changed, value),
                    None => changed,
                };
            }
        }
        flags[flag as usize] = Some(value);
    }
    Ok(flags)
}

impl FlagBase {
    fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        match self {
            Self::Local(source) => body.value(source.condition(condition)),
            Self::Stored(cache) => {
                let canonical = condition.canonical();
                let slot = &mut cache.conditions[condition_index(canonical)];
                let value = if let Some(value) = slot.as_ref() {
                    body.value(value)?
                } else {
                    let value = cpu.read_condition(body, canonical)?;
                    *slot = Some(value.clone());
                    value
                };
                Ok(if condition.is_inverted() {
                    value.eq(0)
                } else {
                    value
                })
            }
        }
    }

    fn read_flags(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        needed: FlagMask,
    ) -> Result<[Option<Val<I1>>; 6], BuildError> {
        match self {
            Self::Local(source) => {
                Ok(StatusFlag::ALL.map(|flag| needed.contains(flag).then(|| source.flag(flag))))
            }
            Self::Stored(cache) => {
                let missing = StatusFlag::ALL.iter().fold(FlagMask::EMPTY, |mask, flag| {
                    if needed.contains(*flag) && cache.flag_slot(*flag).is_none() {
                        mask.union(FlagMask::of(*flag))
                    } else {
                        mask
                    }
                });
                if missing != FlagMask::EMPTY {
                    let flags = cpu.read_flags(body, missing)?;
                    for (flag, value) in StatusFlag::ALL.into_iter().zip(flags) {
                        if value.is_some() {
                            *cache.flag_slot(flag) = value;
                        }
                    }
                }
                Ok(StatusFlag::ALL.map(|flag| {
                    needed
                        .contains(flag)
                        .then(|| cache.flag_slot(flag).as_ref().unwrap().clone())
                }))
            }
        }
    }
}

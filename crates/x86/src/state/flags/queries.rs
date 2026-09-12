//! Logical flag queries, condition shortcuts and demand-driven status composition.

use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1};

use crate::flags::{Condition, Flag, FlagChange, FlagMask, StatusFlag};
use crate::state::Cpu;

use super::{condition_index, direct_location, FlagState, StatusBase, StatusState};

impl FlagState {
    pub(in crate::state) fn read(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        flag: Flag,
    ) -> Result<Val<I1>, BuildError> {
        match flag {
            Flag::Status(flag) => self.status.read_flag(body, cpu, flag),
            _ => self
                .direct
                .read(
                    body,
                    direct_location(flag).expect("non-status flags have direct backing"),
                )
                .map(|value| value.truncate::<I1>()),
        }
    }

    pub(in crate::state) fn read_flags<const N: usize>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        requested: [Flag; N],
    ) -> Result<[Val<I1>; N], BuildError> {
        let needed = requested.iter().fold(FlagMask::EMPTY, |mask, flag| {
            mask.union(FlagMask::of(*flag))
        });
        let mut status = resolve_flags(
            body,
            cpu,
            &mut self.status.base,
            &self.status.updates,
            needed.intersection(FlagMask::STATUS),
        )?;
        for value in status.iter_mut().flatten() {
            *value = body.value(&*value)?;
        }
        let mut values = Flag::ALL.map(|_| None);
        for flag in needed.flags() {
            values[flag.index()] = Some(match flag {
                Flag::Status(flag) => status[flag as usize].take().unwrap(),
                _ => self.read(body, cpu, flag)?,
            });
        }
        Ok(requested.map(|flag| values[flag.index()].as_ref().unwrap().clone()))
    }

    pub(in crate::state) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        self.status.condition(body, cpu, condition)
    }
}

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
        let Some(condition) = flag.condition() else {
            return &mut self.auxiliary;
        };
        &mut self.conditions[condition_index(condition)]
    }
}

impl StatusState {
    pub(super) fn read_flag(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        flag: StatusFlag,
    ) -> Result<Val<I1>, BuildError> {
        if let Some(condition) = flag.condition() {
            return self.condition(body, cpu, condition);
        }
        let flags = resolve_flags(body, cpu, &mut self.base, &self.updates, FlagMask::of(flag))?;
        body.value(flags[flag as usize].as_ref().unwrap())
    }

    pub(super) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        let needed = condition.flags();
        let replacement = self.updates.iter().rposition(|update| {
            update.condition.is_none() && update.status_source(needed).is_some()
        });
        let partial = self
            .updates
            .iter()
            .rposition(|update| {
                update.writes().intersects(needed) && update.status_source(needed).is_none()
            })
            .filter(|index| replacement.is_none_or(|replacement| *index > replacement));
        let (mut value, following) = if let Some(index) = partial {
            let flags = resolve_flags(body, cpu, &mut self.base, &self.updates[..=index], needed)?;
            let value = condition.evaluate(|flag| {
                Ok::<_, BuildError>(flags[flag as usize].as_ref().unwrap().clone())
            })?;
            (value, &self.updates[index + 1..])
        } else if let Some(index) = replacement {
            (
                self.updates[index]
                    .status_source(needed)
                    .unwrap()
                    .condition(condition),
                &self.updates[index + 1..],
            )
        } else {
            (
                self.base.condition(body, cpu, condition)?,
                self.updates.as_slice(),
            )
        };
        // Remaining relevant sources cover every condition dependency, even
        // when they preserve other flags. Their comparison shortcuts remain valid.
        for update in following {
            if let Some(source) = update.status_source(needed) {
                let changed = source.condition(condition);
                value = match &update.condition {
                    Some(predicate) => predicate.select(changed, value),
                    None => changed,
                };
            }
        }
        body.value(value)
    }
}

pub(super) fn resolve_flags(
    body: &mut FunctionBuilder<'_>,
    cpu: &Cpu,
    base: &mut StatusBase,
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

impl StatusBase {
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

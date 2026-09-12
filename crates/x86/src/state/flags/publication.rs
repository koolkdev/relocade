//! Terminal publication selects one record without writing earlier payloads.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1};

use crate::alu::AnyStatusSource;
use crate::flags::FlagMask;
use crate::state::Cpu;

use super::{
    queries::resolve_flags,
    record::{self, FlagRecord},
    FlagState, StatusBase, StatusState,
};

impl FlagState {
    pub(in crate::state) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
    ) -> Result<(), BuildError> {
        self.status.publish(body, cpu)?;
        self.direct.publish(body)
    }
}

impl StatusState {
    fn publish(&self, body: &mut FunctionBuilder<'_>, cpu: &Cpu) -> Result<(), BuildError> {
        let memory = cpu.memory();
        if self.updates.is_empty() {
            return self.base.publish(body, memory);
        }
        let has_partial = self
            .updates
            .iter()
            .any(|update| update.status_source(FlagMask::STATUS).is_none());
        let partial_condition = if has_partial {
            let mut needs_concrete: Val<I1> = false.into();
            for update in &self.updates {
                let partial = update.status_source(FlagMask::STATUS).is_none();
                needs_concrete = match &update.condition {
                    Some(condition) => condition.select(partial, needs_concrete),
                    None => partial.into(),
                };
            }
            let needs_concrete = body.value(needs_concrete)?;
            if needs_concrete.same_expression(&body.value::<I1>(true)?) {
                return self.publish_concrete(body, cpu);
            }
            if needs_concrete.same_expression(&body.value::<I1>(false)?) {
                None
            } else {
                Some(needs_concrete)
            }
        } else {
            None
        };
        // The last active replacement owns the record unless a newer partial
        // change survives. Both paths keep control depth independent of history.
        body.block::<()>(|mut block, done| {
            if let Some(needs_concrete) = partial_condition {
                block.if_(needs_concrete, |mut arm| {
                    self.publish_concrete(&mut arm, cpu)?;
                    arm.branch(&done, ())
                })?;
            }
            for update in self.updates.iter().rev() {
                if let Some(source) = update.status_source(FlagMask::STATUS) {
                    block.if_(update.condition.as_ref().unwrap(), |mut arm| {
                        publish_source(&mut arm, memory, source)?;
                        arm.branch(&done, ())
                    })?;
                }
            }
            self.base.publish(&mut block, memory)
        })
    }

    fn publish_concrete(
        &self,
        body: &mut FunctionBuilder<'_>,
        cpu: &Cpu,
    ) -> Result<(), BuildError> {
        // A publication arm must not put descendant-scoped reads into the live
        // state used by later instructions or exits.
        let mut base = self.base.clone();
        let flags = resolve_flags(body, cpu, &mut base, &self.updates, FlagMask::STATUS)?;
        let status = flags.map(|flag| flag.expect("publication resolves all six status flags"));
        record::write_concrete(body, cpu.memory(), status)
    }
}

impl StatusBase {
    fn publish(&self, body: &mut FunctionBuilder<'_>, memory: Mem) -> Result<(), BuildError> {
        match self {
            Self::Stored(_) => Ok(()),
            Self::Local(source) => publish_source(body, memory, source),
        }
    }
}

fn publish_source(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
    source: &AnyStatusSource,
) -> Result<(), BuildError> {
    match source {
        AnyStatusSource::Byte(source) => FlagRecord::from_source(source).write(body, memory),
        AnyStatusSource::Word(source) => FlagRecord::from_source(source).write(body, memory),
        AnyStatusSource::Dword(source) => FlagRecord::from_source(source).write(body, memory),
    }
}

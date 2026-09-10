//! Terminal publication selects one record without writing earlier payloads.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1};

use crate::flags::{FlagChange, FlagMask, LocalFlagSource};
use crate::state::State;

use super::{
    queries::resolve_flags,
    record::{self, FlagRecord},
    FlagBase,
};

impl State<'_> {
    pub(in crate::state) fn publish_flags(
        &self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<(), BuildError> {
        let memory = self.cpu.memory();
        if self.flags.updates.is_empty() {
            return self.flags.base.publish(body, memory);
        }
        let has_partial = self
            .flags
            .updates
            .iter()
            .any(|update| matches!(update.change, FlagChange::Partial(_)));
        let partial_condition = if has_partial {
            let mut needs_concrete: Val<I1> = false.into();
            for update in &self.flags.updates {
                let partial = matches!(update.change, FlagChange::Partial(_));
                needs_concrete = match &update.condition {
                    Some(condition) => condition.select(partial, needs_concrete),
                    None => partial.into(),
                };
            }
            let needs_concrete = body.value(needs_concrete)?;
            if needs_concrete.same_expression(&body.value::<I1>(true)?) {
                return self.publish_concrete_flags(body);
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
                    self.publish_concrete_flags(&mut arm)?;
                    arm.branch(&done, ())
                })?;
            }
            for update in self.flags.updates.iter().rev() {
                if let FlagChange::Complete(source) = &update.change {
                    block.if_(update.condition.as_ref().unwrap(), |mut arm| {
                        publish_source(&mut arm, memory, source)?;
                        arm.branch(&done, ())
                    })?;
                }
            }
            self.flags.base.publish(&mut block, memory)
        })
    }

    fn publish_concrete_flags(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        // A publication arm must not put descendant-scoped reads into the live
        // state used by later instructions or exits.
        let mut base = self.flags.base.clone();
        let flags = resolve_flags(
            body,
            self.cpu,
            &mut base,
            &self.flags.updates,
            FlagMask::ALL,
        )?;
        let status = flags.map(|flag| flag.expect("publication resolves all six status flags"));
        record::write_concrete(body, self.cpu.memory(), status)
    }
}

impl FlagBase {
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
    source: &LocalFlagSource,
) -> Result<(), BuildError> {
    match source {
        LocalFlagSource::Byte(source) => FlagRecord::from_source(source).write(body, memory),
        LocalFlagSource::Word(source) => FlagRecord::from_source(source).write(body, memory),
        LocalFlagSource::Dword(source) => FlagRecord::from_source(source).write(body, memory),
    }
}

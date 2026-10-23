//! Pending architectural transfers, consumed after successful block completion.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32};

use crate::{execution::ExecutionBuilder, runtime::Runtime};

pub(in crate::execution) enum Transfer {
    Fallthrough(Val<I32>),
    Jump(Val<I32>),
    Branch {
        taken: Val<I1>,
        target: Val<I32>,
        fallthrough: Val<I32>,
    },
    Call(Val<I32>),
    Return(Val<I32>),
    Interrupt(Val<I32>),
}

impl Transfer {
    pub(in crate::execution) fn eip(&self) -> Val<I32> {
        match self {
            Self::Fallthrough(eip)
            | Self::Jump(eip)
            | Self::Call(eip)
            | Self::Return(eip)
            | Self::Interrupt(eip) => eip.clone(),
            Self::Branch {
                taken,
                target,
                fallthrough,
            } => taken.select(target, fallthrough),
        }
    }

    fn emit<F>(
        self,
        mut body: BlockBuilder<'_>,
        mut resolve: impl FnMut(&Val<I32>) -> Result<Option<F>, BuildError>,
        runtime: Runtime,
    ) -> Result<(), BuildError>
    where
        F: for<'body> FnOnce(BlockBuilder<'body>) -> Result<(), BuildError>,
    {
        match self {
            Self::Fallthrough(eip) | Self::Jump(eip) | Self::Call(eip) => {
                enter(body, resolve(&eip)?, runtime)
            }
            Self::Branch {
                taken,
                target,
                fallthrough,
            } => {
                let target = resolve(&target)?;
                let fallthrough = resolve(&fallthrough)?;
                if target.is_some() || fallthrough.is_some() {
                    body.if_(taken, |arm| enter(arm, target, runtime))?;
                }
                enter(body, fallthrough, runtime)
            }
            Self::Return(_) | Self::Interrupt(_) => runtime.dispatch(body),
        }
    }
}

fn enter<F>(
    body: BlockBuilder<'_>,
    destination: Option<F>,
    runtime: Runtime,
) -> Result<(), BuildError>
where
    F: for<'body> FnOnce(BlockBuilder<'body>) -> Result<(), BuildError>,
{
    match destination {
        Some(enter) => enter(body),
        None => runtime.dispatch(body),
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Publishes successful execution and checks its budget before resolving entries.
    /// The module owns destination lookup and emission; this builder keeps branch
    /// conditions, return semantics and mandatory dispatch boundaries.
    pub(crate) fn finish_block<F>(
        mut self,
        resolve: impl FnMut(&Val<I32>) -> Result<Option<F>, BuildError>,
    ) -> Result<(), BuildError>
    where
        F: for<'body> FnOnce(BlockBuilder<'body>) -> Result<(), BuildError>,
    {
        self.publish(true)?;
        if self.requires_dispatch {
            return self.runtime.dispatch(self.body);
        }
        self.transfer.emit(self.body, resolve, self.runtime)
    }
}

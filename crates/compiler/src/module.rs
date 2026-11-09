//! Compile retained functions, then assemble their bodies in declaration order.

mod declarations;
mod emission;
mod functions;

use crate::{CompileExecutor, Program};
use functions::Definitions;
use futures_util::{stream::FuturesUnordered, TryStreamExt};

pub(super) fn encode(program: Program) -> Vec<u8> {
    let mut definitions = Definitions::new(program);
    while let Some(function) = definitions.next() {
        definitions.include(function.place());
    }
    let mut emission = definitions.finish();
    let functions = emission
        .take_functions()
        .into_iter()
        .map(|function| function.emit())
        .collect();
    emission.finish(functions)
}

pub(super) async fn encode_with<E: CompileExecutor>(
    program: Program,
    executor: &E,
) -> Result<Vec<u8>, E::Error> {
    let mut definitions = Definitions::new(program);
    let mut pending = FuturesUnordered::new();
    loop {
        while let Some(function) = definitions.next() {
            pending.push(executor.execute(move || function.place()));
        }
        match pending.try_next().await? {
            Some(function) => definitions.include(function),
            None => break,
        }
    }
    // Surviving calls determine final indices. Every placement must complete
    // before emission can read those immutable mappings.
    let mut emission = definitions.finish();
    let functions = emission
        .take_functions()
        .into_iter()
        .map(|function| executor.execute(move || function.emit()))
        .collect::<FuturesUnordered<_>>()
        .try_collect()
        .await?;
    Ok(emission.finish(functions))
}

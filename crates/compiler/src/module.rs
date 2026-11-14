//! Compile retained functions, then assemble their bodies in declaration order.

mod declarations;
mod emission;
mod functions;

use crate::{BuiltProgram, CompileExecutor};
use functions::Definitions;
use futures_util::{stream::FuturesUnordered, TryStreamExt};

pub(super) fn encode(program: BuiltProgram) -> Vec<u8> {
    let mut definitions = Definitions::new(program);
    while let Some(function) = definitions.next() {
        definitions.include(function.optimize());
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
    program: BuiltProgram,
    executor: &E,
) -> Result<Vec<u8>, E::Error> {
    let mut definitions = Definitions::new(program);
    let mut pending = FuturesUnordered::new();
    loop {
        while let Some(function) = definitions.next() {
            pending.push(executor.execute(move || function.optimize()));
        }
        match pending.try_next().await? {
            Some(function) => definitions.include(function),
            None => break,
        }
    }
    // Surviving calls determine final indices. Every optimization must complete
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

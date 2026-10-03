//! Logical result shapes and signature-directed arguments.
use crate::{value::ValueSource, Argument, BlockBuilder, BuildError, Type, Val, ValueType};

mod sealed {
    use super::*;

    pub trait Shape {}

    pub trait Values: Sized {
        fn types() -> Vec<Type>;
        /// Requests scalar sources in result order, using their logical types.
        fn bind(next: &mut dyn FnMut(Type) -> ValueSource) -> Self;
    }
}

/// The logical shape of call and control results, and of loop inputs.
///
/// A scalar marker such as `I32` or `F64` produces its typed `Val`. `()` produces no values.
/// Tuples of up to eight shapes produce corresponding tuples of typed values;
/// arrays repeat a shape, and shapes may be nested. Components retain their
/// logical types even when several types use the same WebAssembly carrier.
///
/// Control-result components are demanded independently. An unused component
/// can omit its loads, pure calls and possible traps; ordered arm effects remain.
/// Function invocations follow [`BlockBuilder::call`]: when a call runs,
/// its callee evaluates every declared result, including discarded components.
pub trait Results: sealed::Shape {
    type Values: sealed::Values;
}

impl<T: ValueType> Results for T {
    type Values = Val<T>;
}

impl<T: ValueType> sealed::Shape for T {}

impl<T: ValueType> sealed::Values for Val<T> {
    fn types() -> Vec<Type> {
        vec![T::TYPE]
    }

    fn bind(next: &mut dyn FnMut(Type) -> ValueSource) -> Self {
        Val::from_source(next(T::TYPE))
    }
}

impl Results for () {
    type Values = ();
}

impl sealed::Shape for () {}

impl sealed::Values for () {
    fn types() -> Vec<Type> {
        vec![]
    }
    fn bind(_: &mut dyn FnMut(Type) -> ValueSource) {}
}

impl<R: Results, const N: usize> Results for [R; N] {
    type Values = [R::Values; N];
}

impl<R: Results, const N: usize> sealed::Shape for [R; N] {}

impl<V: sealed::Values, const N: usize> sealed::Values for [V; N] {
    fn types() -> Vec<Type> {
        V::types().repeat(N)
    }

    fn bind(next: &mut dyn FnMut(Type) -> ValueSource) -> Self {
        std::array::from_fn(|_| V::bind(next))
    }
}

/// Values or native literals supplied to a return, control result, loop entry or branch label.
///
/// A scalar argument supplies one component, `()` supplies none, and a tuple
/// or array supplies its components in order. A vector supplies a runtime-sized
/// list of scalar arguments. The logical signature validates their number,
/// types, body ownership and visibility.
pub struct Arguments(pub(super) Vec<Argument>);

impl<T: Into<Argument>> From<T> for Arguments {
    fn from(value: T) -> Self {
        Self(vec![value.into()])
    }
}

impl From<()> for Arguments {
    fn from(_: ()) -> Self {
        Self(vec![])
    }
}

impl<T: Into<Arguments>, const N: usize> From<[T; N]> for Arguments {
    fn from(values: [T; N]) -> Self {
        Self(
            values
                .into_iter()
                .flat_map(|value| value.into().0)
                .collect(),
        )
    }
}

impl<T: Into<Argument>> From<Vec<T>> for Arguments {
    fn from(values: Vec<T>) -> Self {
        Self(values.into_iter().map(Into::into).collect())
    }
}

impl BlockBuilder<'_> {
    pub(super) fn result_arguments(
        &self,
        arguments: impl Into<Arguments>,
        types: &[Type],
    ) -> Result<Vec<usize>, BuildError> {
        let arguments = arguments.into().0;
        check_count(types.len(), arguments.len())?;
        arguments
            .into_iter()
            .zip(types)
            .map(|(argument, &ty)| self.argument(argument, ty))
            .collect()
    }
}

pub(super) fn check_count(expected: usize, actual: usize) -> Result<(), BuildError> {
    if expected == actual {
        Ok(())
    } else {
        Err(BuildError::ResultCount { expected, actual })
    }
}

macro_rules! tuples {
    ($($shape:ident $index:tt),+) => {
        impl<$($shape: Results),+> Results for ($($shape,)+) {
            type Values = ($($shape::Values,)+);
        }

        impl<$($shape: Results),+> sealed::Shape for ($($shape,)+) {}

        impl<$($shape: sealed::Values),+> sealed::Values for ($($shape,)+) {
            fn types() -> Vec<Type> {
                let mut types = Vec::new();
                $(types.extend($shape::types());)+
                types
            }

            fn bind(next: &mut dyn FnMut(Type) -> ValueSource) -> Self {
                ($($shape::bind(next),)+)
            }
        }

        impl<$($shape: Into<Arguments>),+> From<($($shape,)+)> for Arguments {
            fn from(values: ($($shape,)+)) -> Self {
                let mut arguments = Vec::new();
                $(arguments.extend(values.$index.into().0);)+
                Self(arguments)
            }
        }
    };
}

tuples!(A 0);
tuples!(A 0, B 1);
tuples!(A 0, B 1, C 2);
tuples!(A 0, B 1, C 2, D 3);
tuples!(A 0, B 1, C 2, D 3, E 4);
tuples!(A 0, B 1, C 2, D 3, E 4, F 5);
tuples!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuples!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

pub(super) fn types<R: Results>() -> Vec<Type> {
    <R::Values as sealed::Values>::types()
}

pub(super) fn bind<R: Results>(body: &BlockBuilder<'_>, outputs: &[usize]) -> R::Values {
    let mut outputs = outputs.iter().copied();
    bind_sources::<R>(|_| {
        ValueSource::from_definition(
            body.arena.clone(),
            Ok(outputs
                .next()
                .expect("the declared result shape has an output")),
        )
    })
}

pub(crate) fn bind_sources<R: Results>(mut next: impl FnMut(Type) -> ValueSource) -> R::Values {
    <R::Values as sealed::Values>::bind(&mut next)
}

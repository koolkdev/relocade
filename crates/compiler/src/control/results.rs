//! Logical result shapes and signature-directed arguments.
use crate::{Argument, FunctionBuilder, IntType, Type, Val};

mod sealed {
    use super::*;

    pub trait Shape {}

    pub trait Values: Sized {
        fn types() -> Vec<Type>;
        fn bind(body: &FunctionBuilder<'_>, outputs: &mut dyn Iterator<Item = usize>) -> Self;
    }
}

/// The logical result shape of a block, conditional or switch.
///
/// An integer marker such as `I32` produces `Val<I32>`. `()` produces no values.
/// Tuples of up to eight shapes produce corresponding tuples of typed values;
/// shapes may be nested to describe larger results. Components retain their
/// logical types even when several types use the same WebAssembly carrier.
pub trait Results: sealed::Shape {
    type Values: sealed::Values;
}

impl<T: IntType> Results for T {
    type Values = Val<T>;
}

impl<T: IntType> sealed::Shape for T {}

impl<T: IntType> sealed::Values for Val<T> {
    fn types() -> Vec<Type> {
        vec![T::TYPE]
    }

    fn bind(body: &FunctionBuilder<'_>, outputs: &mut dyn Iterator<Item = usize>) -> Self {
        Val::new(
            body.arena.clone(),
            Ok(outputs
                .next()
                .expect("the declared result shape has an output")),
        )
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
    fn bind(_: &FunctionBuilder<'_>, _: &mut dyn Iterator<Item = usize>) {}
}

/// Values or native literals supplied to a block result or branch label.
///
/// A scalar argument supplies one result, `()` supplies none, and a tuple
/// supplies the concatenated arguments of its components. The enclosing logical
/// result signature validates the number, types, body ownership and visibility.
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

            fn bind(body: &FunctionBuilder<'_>, outputs: &mut dyn Iterator<Item = usize>) -> Self {
                ($($shape::bind(body, outputs),)+)
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

pub(super) fn bind<R: Results>(body: &FunctionBuilder<'_>, outputs: &[usize]) -> R::Values {
    <R::Values as sealed::Values>::bind(body, &mut outputs.iter().copied())
}

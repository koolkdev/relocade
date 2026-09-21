//! Independently selectable frontends, with the same cases in both engines.

#[derive(Clone, Copy, Debug)]
pub(crate) enum Frontend {
    Block,
    Interpreter,
}

macro_rules! test_frontends {
    ($group:ident, $cases:expr, $check:path) => {
        mod $group {
            use super::*;
            use $crate::support::{execution::Frontend, step::Engine};

            #[test]
            fn block() {
                $check(&($cases), Engine::Wasmtime, Frontend::Block);
            }

            #[test]
            fn interpreter() {
                $check(&($cases), Engine::Wasmtime, Frontend::Interpreter);
            }

            #[test]
            #[ignore = "requires Node.js; run the explicit V8 lane"]
            fn v8_block() {
                $check(&($cases), Engine::V8, Frontend::Block);
            }

            #[test]
            #[ignore = "requires Node.js; run the explicit V8 lane"]
            fn v8_interpreter() {
                $check(&($cases), Engine::V8, Frontend::Interpreter);
            }
        }
    };
}

pub(crate) use test_frontends;

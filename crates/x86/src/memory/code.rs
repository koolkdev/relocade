//! Code-write notifications are effects of actual stores, never access probes.

use wasm86_compiler::{
    BlockBuilder, BuildError, Func, FunctionImport, Program, Signature, Type, Val, I1, I32,
};

/// Host-owned code watch in a virtual page entry or physical mapping kind word.
/// It never changes architectural permissions. All writable aliases of a watched
/// backing page carry this bit before a snapshot is copied.
pub const CODE_WATCH: u32 = 0x10;

#[derive(Clone, Copy)]
pub(super) struct CodeWrites(Func);

impl CodeWrites {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self(program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "invalidateCode".into(),
            signature: Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![],
            },
        }))
    }

    /// The callback invalidates dependent code before the store. It may clear
    /// watch bits, but cannot change mappings, CPU state or guest bytes.
    pub(super) fn before_write(
        self,
        body: &mut BlockBuilder<'_>,
        watched: &Val<I1>,
        address: &Val<I32>,
        bytes: u32,
    ) -> Result<(), BuildError> {
        body.if_(watched, |mut slow| {
            slow.call::<()>(self.0, &[address.into(), bytes.into()])
        })
    }
}

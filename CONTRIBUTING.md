# Contributing

Use Intel's architecture manuals to establish instruction behavior. Tests should
use literal expectations or an independent model. The two x86 frontends share
semantics, so agreement between them is not an independent correctness check.
Repository conventions and review requirements are in [AGENTS.md](AGENTS.md).

## Build and test

The ordinary suite runs generated Wasm in Wasmtime and needs no Node.js:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo doc --workspace --no-deps --locked
```

Ignored engine tests use Node.js 24 on `PATH` and run V8 with TurboFan. Select the
affected suite for a focused check, for example:

```sh
cargo test -p wasm86-x86 --lib --locked -- --ignored segments::pointers
cargo test -p wasm86-compiler --test integration --locked -- --ignored constant_control
```

`cargo test --workspace --locked -- --ignored` runs the full V8 lane when needed.
Compare generated Wasm bytes before timing a change; measure changed output with
representative V8 workloads and matching execution boundaries. Wasmtime results
establish correctness, not V8 performance.

## Adding an instruction

1. Add its forms and semantic handler to the relevant module in
   [instruction/definitions](crates/x86/src/instruction/definitions). A new family
   module also joins `opcode_forms` in [definitions.rs](crates/x86/src/instruction/definitions.rs).
2. Use shared execution operations for checked operands, state changes and exits.
   Pure integer results and flag changes belong in [alu](crates/x86/src/alu);
   access rules belong in memory and segment owners. Complete fault checks before
   changing the instruction's architectural state; REP does this per element.
3. Add behavioral cases to the instruction's [test suite](crates/x86/tests/suites).
   Cover widths, aliases, memory access and relevant restart boundaries. Keep
   encoding rejection, ABI and generated-code assertions in focused tests.

Both decoders consume the same declarations. For example, the existing SHL family
connects its encodings to one handler:

```rust
instruction_families! {
    SHL {
        execute: shift(ShiftOp::Left);
        forms {
            0xD0 /4 => byte(rm, constant(1));
            0xD1 /4 => word_or_dword(rm, constant(1));
            0xD2 /4 => byte(rm, CL);
            0xD3 /4 => word_or_dword(rm, CL);
            0xC0 /4 => byte(rm, imm8);
            0xC1 /4 => word_or_dword(rm, imm8);
        }
    }
}
```

Operands are in handler argument order. `rm` binds ModRM.r/m, `/4` reserves
ModRM.reg as an opcode extension, and `word_or_dword` follows the effective operand
size. Use `execute: handler::<_>;` when the row must explicitly supply a logical
width that cannot be inferred from the handler's operands. See
[shifts.rs](crates/x86/src/instruction/definitions/shifts.rs) for the complete
handler and neighboring families for other forms. Declaration validation checks
field compatibility; catalog tests reject overlapping encodings.

Forms derive ordinary operand effects. Declare additional effects when accesses
are implicit, as in [strings](crates/x86/src/instruction/definitions/strings.rs),
or when a segment load terminates execution under the current assumptions, as in
[segment instructions](crates/x86/src/instruction/definitions/segments.rs).

## Writing tests

An `InstructionCase` states encoding bytes, initial state and expected changes.
Register its case list with `test_cases!`; `SequenceCase` and `test_sequences!`
cover several instructions with a checkpoint after each one. These run through
snapshot and interpreter entries, ordinarily with flat and segmented profiles,
in Wasmtime and the explicit V8 lane. The x86 suites live in the library test
target so they can observe private state readers without adding public APIs.

Give complete parent-register expectations for narrow writes. Unlisted registers
and guest bytes must remain unchanged. Choose flag expectations deliberately:
`Preserved` checks the prior logical value, `Undefined` accepts an architectural
undefined result, and `preserving_flags` checks the whole incoming backing record.
Use focused tests to protect wasm86's chosen undefined-result policy itself.

Cases default to EIP `0x1000`, one retired instruction and fallthrough dispatch.
Specify a different dispatch or fault explicitly. An ordinary instruction fault
preserves its entry state and does not retire it; REP faults retain completed
elements, and sequences preserve earlier instructions. Use segment caches and
`segmented_only()` for cases outside flat assumptions. Snapshot fixtures validate
instruction-fetch spans before entry.

The [case builder](crates/x86/tests/support/cases.rs),
[sequence builder](crates/x86/tests/support/sequences.rs) and existing suites show
memory mappings, flag rules, partial expectations and repeated execution. Keep
expected results independent of the implementation under test.

Compiler tests use [Fixture](crates/compiler/tests/support/fixture.rs) to build
modules with imported memories and callbacks, then inspect results, memory and
recorded calls. Use `fixture.program` for multiple functions and `module.bytes()`
for Wasm shape checks. Suites are registered in
[integration.rs](crates/compiler/tests/integration.rs); owner-local tests cover
private invariants.

## Documentation ownership

Keep the README focused on orientation and getting started. Public API contracts
belong in rustdoc. The [host integration guide](crates/x86/docs/host-integration.md)
is also included in the x86 crate documentation; edit that single source for ABI
changes. Compatibility choices and non-obvious implementation decisions belong
beside their owner, with a focused test where appropriate. Adding an instruction
normally updates its definition and tests; it does not require another prose
description of ordinary x86 behavior in the README or interpreter entry docs.

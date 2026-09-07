# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-x86` compiles 32-bit MOV blocks from byte snapshots:

```rust
let block = wasm86_x86::compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
```

The requested instruction count is exact. Missing or unsupported selected bytes
are construction errors; bytes after the selection are ignored. The returned
module exports `block_1000` and imports `wasm86.cpuState` memory (minimum one
64-KiB page) and `wasm86.dispatch(i32) -> i64`. CPU state uses little-endian 32-bit
fields: EAX through EDI in encoding order at offsets 24–52, EIP at 56, and the
completed-instruction count at 144. Final register writes retain first-write
order, then EIP and count are updated with 32-bit wrapping arithmetic. The block
tail-calls dispatch with the next EIP and returns its result. This snapshot path
supports B8–BF with imm32 operands and 89/8B with register operands
(ModRM.mod = 3). A selected memory-operand ModRM is a construction error.

`compile_interpreter_step()` builds a generated `step() -> i64` entry for the
same unprefixed MOV32 subset. Both compiler functions return a `CompiledModule`
containing WebAssembly bytes and its exported entry name.

```rust
let module = wasm86_x86::compile_interpreter_step()?;
```

The step reads EIP from CPU state and fetches the instruction from paged guest
memory. A successful five-byte contiguous-range check permits direct reads of
the selected instruction. Otherwise the exact path checks the opcode first and
reads only the fields it requires. It uses checked byte reads when an immediate
cannot be read directly. Success uses the same MOV semantics, state publication
and dispatch as snapshot blocks.

Snapshot decoding reads supplied bytes while compiling; runtime decoding reads
guest bytes during execution. Both use shared instruction forms for opcode
patterns, physical fields and operand binding, then pass decoded operands to
shared instruction lowering.
Execution drivers choose how to fetch and when to publish CPU state. Pending register
writes can be published into a terminating fault branch without consuming the
parent state used by the successful path. A location-based value environment
forwards known register definitions and caches reads. Computed register accesses
synchronize overlapping definitions to backing, then invalidate potentially
written locations. These are completed effects; publication does not undo a
partially executed instruction.

In addition to `cpuState` and `dispatch`, the module imports `wasm86.guest`
(minimum one Wasm page) and `wasm86.machine` (minimum 64 pages, or 4 MiB).
All three memory imports require distinct backing objects. Machine memory starts
with 2^20 little-endian 32-bit page-table entries: bit 0 marks presence and bits
12–31 identify a 4-KiB frame in guest memory. Present frames must fit the backing
RAM; invalid backing is a Wasm trap, not a guest page fault.

A missing instruction page returns the 64-bit word
`(4 << 48) | (0x10 << 32) | first_unavailable_address`. An unsupported opcode
or memory-operand ModRM returns `(8 << 48) | (opcode << 32) | instruction_eip`.
This reports the current implementation's unsupported subset, not an
architectural invalid-opcode fault.
The diagnostic includes the opcode, not the ModRM byte.
Both exits preserve CPU state and skip dispatch. The entry executes one
instruction and has no prefix, instruction-budget or run-loop behavior.

`wasm86-compiler` builds scalar WebAssembly functions from integer constants,
parameters and typed integer expressions. Values such as `Val<I1>` and `Val<I32>` carry
logical integer types; function signatures use the corresponding `Type` variants.
Supported integer sizes are 1, 8, 16, 32 and 64 bits. Values support fluent
expressions such as `value.add(1)`. Calling `body.return_(&value)` completes the
function body; shared expressions use reusable WebAssembly locals.
At a return, the signature supplies the logical type, so `body.return_(7)` is
valid too. Use `body.value::<I32>(operand)?` when a value must be retained or used to
start a symbolic expression; it accepts a literal or an existing typed value.

Expressions support wrapping addition, bitwise `and`/`or`, constant-count `shl`,
and `eq`/`ne` predicates that return `Val<I1>`. The borrowed `unsigned()` view
provides `shr`, `lt`, `ge` and zero extension, for example
`byte.unsigned().extend::<I32>().shl(8)`. `truncate::<I8>()` retains the low eight
bits. Rust checks conversion direction; conversions to the same type are allowed.
Shift counts are modulo 32 for I1/I8/I16/I32 and modulo 64 for I64, so shifting an
I8 by 8 produces zero and shifting it by 32 preserves its value. Comparisons,
unsigned right shifts and widening read the logical low bits, including after
arithmetic that overflows a narrow type.

Build conditional code with the same builder methods:

```rust
body.if_(value.eq(0), |mut branch| {
    branch.store::<I32>(memory, 12, 9)?;
    branch.return_(7)
})?;
body.return_(value.add(1))?;
```

A branch can load, store, contain nested `if_` calls, return, or tail-call. Ending
its closure with `Ok(())` without a terminal lets it fall through. A false
condition skips the branch. Child reads, call results, joined values and
expressions depending on them cannot be consumed outside that child; pure
expressions from parent values remain usable. Reads preserve snapshots across
conditional stores, which can require capturing a read before the condition.

Use `if_value` to obtain a value from the selected branch:

```rust
let selected = body.if_value::<I32>(value.eq(0),
    |arm| arm.yield_(7),
    |arm| arm.yield_(value.add(1)),
)?;
body.return_(selected.add(2))?;
```

`yield_` consumes the direct value-arm builder and supplies the conditional's
result; `return_` still returns from the whole function. Every arm must yield,
return or tail-call, and at least one arm must yield. Arm effects remain ordered,
while an unused yielded expression and its possible traps can disappear. A join
preserves raw intermediate bits; narrow values are normalized when an operation
or function boundary needs their logical low bits. Construction errors discard
both arms and leave the parent usable.

Functions can also finish with `body.tail_call(target, &[value.argument()])`.
The target may be imported or defined. Use `Val::argument()` or `.into()` to combine values and literals
in one argument list; the call checks them against its signature.
`Program::import_function` takes a `FunctionImport` containing the module name,
field name and logical `Signature`. Unused function imports are omitted; a direct
export also retains an imported function.

For a call that returns to the current function, use
`body.call::<I32>(helper, &[value.argument(), 7.into()])?`. Its typed result can
be shared by later expressions. A result created inside a branch stays within
that branch and its descendants. The compiler conservatively infers which
memory bytes defined helpers may read or write: calls without writes may be
deferred or omitted when unused, including their possible traps. Calls that may
write, imported calls and unresolved recursive calls execute in authored order
even when unused.

WebAssembly carries 1-, 8- and 16-bit integers in `i32`. Narrow arguments must have
their unused upper bits clear, and returned narrow values satisfy the same rule.
A one-bit argument or result is therefore `0` or `1`. The compiler normalizes
narrow call arguments once per shared value without masking intermediate
arithmetic. Imported functions must return narrow results with their unused
upper bits clear too, including when the import is exported directly.

Fixed-offset memory access supports `I8`, `I16`, `I32` and `I64`, using 1, 2, 4
and 8 bytes respectively. `body.load::<I32>(memory, offset)` reads a snapshot;
`body.store(memory, offset, &value)` writes the value at that point in the body.
Literal operands work directly: `body.store::<I32>(memory, 12, 9)`.
Stores keep their authored order, used loads preserve their value across writes,
and unused loads are omitted. Distinct memory declarations require distinct
backing memories.

For computed addresses, use `body.load_at::<I8>(memory, &address, offset)` and
`body.store_at(memory, &address, offset, &value)`, where `address` is a `Val<I32>`
or an integer literal.
The unsigned address plus the constant displacement does not wrap. To wrap an
address calculation at 32 bits, build it explicitly with `address.add(amount)`.
Reads preserve their snapshots even when their addresses depend on other reads;
unused reads and their unused address computations are omitted.

Run the Rust checks with:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The V8 execution tests require Node.js 24 on `PATH`. Run both the default and
optimizing V8 modes with:

```sh
cargo test --workspace --locked -- --ignored
```

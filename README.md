# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-x86` compiles byte, word and dword MOV blocks from byte snapshots:

```rust
let block = wasm86_x86::compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
```

The requested instruction count is exact. Missing, overlong or unsupported
selected instructions are construction errors; bytes after the selection are ignored. The returned
module exports `block_1000` and imports `wasm86.cpuState` memory (minimum one
64-KiB page) and `wasm86.dispatch(i32) -> i64`. CPU state uses little-endian 32-bit
fields: EAX through EDI in encoding order at offsets 24–52, EIP at 56, and the
completed-instruction count at 144. Final dirty views retain first-write order,
then EIP and count are updated with 32-bit wrapping arithmetic. The block
tail-calls dispatch with the next EIP and returns its result. This snapshot path
supports these MOV forms in default-32 operand and address mode:

| Operands | Byte | Word (`66`) | Dword |
| --- | --- | --- | --- |
| Opcode-selected register and immediate | B0–B7 | B8–BF | B8–BF |
| Register and register/memory | 88/8A | 89/8B | 89/8B |
| Register/memory destination and immediate | C6 /0 | C7 /0 | C7 /0 |
| Accumulator and absolute memory offset | A0/A2 | A1/A3 | A1/A3 |

The `66` operand-size prefix selects word data; repeating it keeps that size.
Byte forms remain byte-sized with `66`. Other prefixes, including address-size
`67`, are outside the supported subset. The fifteen-byte instruction limit
includes every prefix, opcode and required operand field.

Byte register codes select AL/CL/DL/BL/AH/CH/DH/BH. Word codes select the low
sixteen bits of EAX through EDI. Byte and word writes preserve the other bits
of the parent register. Memory addresses use 32-bit ModRM/SIB base, index,
scale and displacement fields, or a 32-bit absolute offset. A0/A2 use AL;
A1/A3 use AX with `66` and EAX otherwise. Their encoded address is always four
bytes, independent of the data width.
Effective-address sums wrap at 32 bits; both frontends use flat addresses and
ignore segment bases. Blocks containing only register operands retain just the
CPU and dispatch imports.

`compile_interpreter_step()` builds a generated `step() -> i64` entry for the
same MOV subset. Both compiler functions return a `CompiledModule`
containing WebAssembly bytes and its exported entry name.

```rust
let module = wasm86_x86::compile_interpreter_step()?;
```

The step reads EIP from CPU state and fetches the instruction from paged guest
memory. A successful five-byte contiguous-range check permits direct reads of
fields within that window; later fields use checked fetch. Otherwise the exact
path checks the opcode first and reads only required fields, checking bytes in
order when a word or dword cannot be read directly. C6/C7 read ModRM and reject an
unsupported extension before fetching any SIB, displacement or immediate. For
/0, all instruction fields are fetched before any data access is checked.
No read requests byte sixteen. A wide field crossing the limit is read byte
by byte: a missing required byte below the limit faults first. If those bytes
are available, requiring byte sixteen returns general protection with error code
zero. Snapshot decoding follows the same byte order but reports truncation
or `InstructionTooLong` as construction errors. Success uses the same MOV
semantics, state publication and dispatch as snapshot blocks.

Snapshot decoding reads supplied bytes while compiling; runtime decoding reads
guest bytes during execution. Both use shared instruction forms for opcode
patterns, physical fields and operand binding, then pass decoded operands to
shared instruction lowering. The runtime decoder owns its byte cursor, proven
window and completion policy. An execution builder resolves operand locations,
checks memory access, and tracks instruction progress. Shared MOV semantics reads
the source and writes the destination through that builder. A fault publishes
completed register writes into its terminating branch without consuming the
parent state used by the successful path. One value environment tracks typed
byte, word and dword locations, forwarding known definitions and caching reads.
Reads through overlapping views synchronize earlier definitions to backing. A covering
write replaces superseded definitions. Computed register accesses synchronize
overlapping definitions, then invalidate potentially written locations. These
are completed effects; publication does not undo a partially executed instruction.

The step and snapshot blocks with memory operands also import `wasm86.guest`
(minimum one Wasm page) and `wasm86.machine` (minimum 64 pages, or 4 MiB).
All three memory imports require distinct backing objects. Machine memory starts
with 2^20 little-endian 32-bit page-table entries: bit 0 marks presence, bit 1
permits writes, and bits 12–31 identify a 4-KiB frame in guest memory. Data reads
require presence. Present frames must fit the backing RAM; invalid backing is a
Wasm trap, not a guest page fault.

A missing instruction page returns the 64-bit word
`(4 << 48) | (0x10 << 32) | first_unavailable_address`. A data fault returns
`(4 << 48) | (error << 32) | first_denied_address`, where error bit 1 identifies a
write and bit 0 identifies a present but denied page. A word or dword data range
that crosses `0xffffffff` is rejected at its start with read error 0 or write error 2;
this is the current address-space policy. A one-byte access at that address fits
without consulting another page. Instruction fetch instead wraps.
All pages are checked before a data store writes any byte, including scattered
physical backing. A fault publishes earlier completed instructions and leaves EIP
at the faulting instruction; the failed instruction does not retire or dispatch.

An unsupported instruction form returns `(8 << 48) | (opcode << 32) | instruction_eip`.
The opcode field contains the first byte after any `66` prefixes. This reports
the implementation's unsupported subset, not an architectural invalid-opcode
fault. An instruction requiring more than fifteen bytes returns `2 << 48`, the
zero-error general-protection word, without retiring or dispatching. The step
executes one instruction and has no instruction-budget, segment or run-loop
behavior.

`wasm86-compiler` builds scalar WebAssembly functions from integer constants,
parameters and typed integer expressions. Values such as `Val<I1>` and `Val<I32>` carry
logical integer types; function signatures use the corresponding `Type` variants.
Supported integer sizes are 1, 8, 16, 32 and 64 bits. Values support fluent
expressions such as `value.add(1)`. Calling `body.return_(&value)` completes the
function body; shared expressions use reusable WebAssembly locals.
At a return, the signature supplies the logical type, so `body.return_(7)` is
valid too. Use `body.value::<I32>(operand)?` when a value must be retained or used to
start a symbolic expression; it accepts a literal or an existing typed value.

Use `program.function(signature, |body| { ... })?` to declare and complete a
function together. The handle is returned only after the callback completes its
body; an error or missing completion removes the new declaration. Separate
`declare` and `define` remain available for forward references and recursion.

Expressions support wrapping addition, bitwise `and`/`or`, `shl`,
and `eq`/`ne` predicates that return `Val<I1>`. The borrowed `unsigned()` view
provides `shr`, `lt`, `ge` and zero extension, for example
`byte.unsigned().extend::<I32>().shl(8)`. `truncate::<I8>()` retains the low eight
bits. The borrowed `signed()` view provides sign extension, such as
`displacement.signed().extend::<I32>()`. Rust checks conversion direction;
conversions to the same type are allowed. Left shifts accept an I32 value or a
literal count; unsigned right shifts currently take literal counts.
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
condition skips the branch. `body.if_else(condition, then_arm, else_arm)` builds
two ordinary branches; each can fall through or terminate the function.
Child reads, call results, joined values and expressions depending on them cannot
be consumed outside that child; pure
expressions from parent values remain usable. Reads preserve snapshots across
conditional stores, which can require capturing a read before the condition.

Use `condition.select(when_true, when_false)` for a pure value choice. Both
alternatives are eager inputs, so select does not guard a load or call. Shared
inputs and the selection itself follow normal value placement. Two literals can
specify their type with `condition.select::<I32>(7, 9)`.

Use `if_value` to execute only the selected branch and obtain its value:

```rust
let branch_value = body.if_value::<I32>(value.eq(0),
    |arm| arm.yield_(7),
    |arm| arm.yield_(value.add(1)),
)?;
body.return_(branch_value.add(2))?;
```

`yield_` consumes the direct value-arm builder and supplies the conditional's
result; `return_` still returns from the whole function. Every arm must yield,
return or tail-call, and at least one arm must yield. Arm effects remain ordered,
while an unused yielded expression and its possible traps can disappear. A join
preserves raw intermediate bits; narrow values are normalized when an operation
or function boundary needs their logical low bits. Construction errors discard
both arms and leave the parent usable.

Functions can also finish with `body.tail_call(target, &[value.argument()])`.
The target may be imported or defined. Use `Val::argument()` or `.into()` to
combine values and literals in one argument list; the call checks them against
its signature.
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

# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-x86` compiles MOV, ADD, SUB, CMP, AND, OR, XOR, TEST and SETcc blocks from byte snapshots:

```rust
let block = wasm86_x86::compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
```

The requested instruction count is exact. Missing, overlong or unsupported
selected instructions are construction errors; bytes after the selection are ignored. The returned
module exports `block_1000` and imports `wasm86.cpuState` memory (minimum one
64-KiB page) and `wasm86.dispatch(i32) -> i64`. CPU state uses little-endian 32-bit
fields: EAX through EDI in encoding order at offsets 24–52, EIP at 56, and the
completed-instruction count at 144. An exit publishes its current flag source, then
dirty registers in first-write order. EIP and count use 32-bit wrapping arithmetic. The block
tail-calls dispatch with the next EIP and returns its result. This snapshot path
supports these forms in default-32 operand and address mode:

| Operands | Byte | Word (`66`) | Dword |
| --- | --- | --- | --- |
| MOV opcode-selected register and immediate | B0–B7 | B8–BF | B8–BF |
| MOV register and register/memory | 88/8A | 89/8B | 89/8B |
| MOV register/memory destination and immediate | C6 /0 | C7 /0 | C7 /0 |
| MOV accumulator and absolute memory offset | A0/A2 | A1/A3 | A1/A3 |
| ADD register and register/memory | 00/02 | 01/03 | 01/03 |
| ADD accumulator and immediate | 04 | 05 | 05 |
| CMP register and register/memory | 38/3A | 39/3B | 39/3B |
| CMP accumulator and immediate | 3C | 3D | 3D |
| ADD register/memory and immediate | 80 /0 | 81/83 /0 | 81/83 /0 |
| CMP register/memory and immediate | 80 /7 | 81/83 /7 | 81/83 /7 |
| SUB register and register/memory | 28/2A | 29/2B | 29/2B |
| SUB accumulator and immediate | 2C | 2D | 2D |
| SUB register/memory and immediate | 80 /5 | 81/83 /5 | 81/83 /5 |
| AND register and register/memory | 20/22 | 21/23 | 21/23 |
| AND accumulator and immediate | 24 | 25 | 25 |
| AND register/memory and immediate | 80 /4 | 81/83 /4 | 81/83 /4 |
| OR register and register/memory | 08/0A | 09/0B | 09/0B |
| OR accumulator and immediate | 0C | 0D | 0D |
| OR register/memory and immediate | 80 /1 | 81/83 /1 | 81/83 /1 |
| XOR register and register/memory | 30/32 | 31/33 | 31/33 |
| XOR accumulator and immediate | 34 | 35 | 35 |
| XOR register/memory and immediate | 80 /6 | 81/83 /6 | 81/83 /6 |
| TEST register/memory and register | 84 | 85 | 85 |
| TEST accumulator and immediate | A8 | A9 | A9 |
| TEST register/memory and immediate | F6 /0 | F7 /0 | F7 /0 |
| SETcc register/memory destination | 0F 90–9F | — | — |

Group `83` sign-extends its encoded byte immediate to the operand width. SETcc
writes a byte containing 0 or 1; its ModRM.reg field is ignored.

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
same instruction subset. Both compiler functions return a `CompiledModule`
containing WebAssembly bytes and its exported entry name.

```rust
let module = wasm86_x86::compile_interpreter_step()?;
```

The step reads EIP from CPU state and fetches the instruction from paged guest
memory. A successful five-byte contiguous-range check permits direct reads of
fields within that window; later fields use checked fetch. Otherwise the exact
path checks the opcode first and reads only required fields, checking bytes in
order when a word or dword cannot be read directly. Group instructions read ModRM
and reject unsupported extensions before fetching any SIB, displacement or immediate.
For supported forms, all instruction fields are fetched before data access is checked.
No read requests byte sixteen. A wide field crossing the limit is read byte
by byte: a missing required byte below the limit faults first. If those bytes
are available, requiring byte sixteen returns general protection with error code
zero. Snapshot decoding follows the same byte order but reports truncation
or `InstructionTooLong` as construction errors. Success uses the same instruction
semantics, state publication and dispatch as snapshot blocks.

Snapshot decoding reads supplied bytes while compiling; runtime decoding reads
guest bytes during execution. Both use shared instruction forms for opcode
patterns, physical fields and operand binding, then pass decoded operands to
shared instruction lowering. The runtime decoder owns its byte cursor, proven
window and completion policy. Every opcode map uses the same catalog-driven
switch and selects operand decoding by encoding. Only the chosen opcode checks
its ModRM extension. Direct, checked and prefixed entries share field handling.
Memory decoding selects the SIB or ordinary layout
before reading address fields; ordinary addresses have no index term. Direct
entries retain the original fetch window across fields whose bounds fit it.
An execution builder resolves operand locations,
checks memory access, and tracks instruction progress. Binary instructions have
left and right operands; their operation determines which effects are applied.
The builder's `update` checks write permission before reading the old left value,
then runs the semantic callback and stores its result through the same checked
access. ADD, SUB, AND, OR and XOR use this operation. CMP and TEST only read
operands and set flags, so their memory operands require no write permission.
A fault publishes completed definitions into its terminating branch without
consuming the parent state used by the successful path.

The register value environment tracks typed byte, word and dword locations,
forwarding known definitions and caching reads.
Reads through overlapping views synchronize earlier definitions to backing. A covering
write replaces superseded definitions. Computed register accesses synchronize
overlapping definitions, then invalidate potentially written locations. These
are completed effects; publication does not undo a partially executed instruction.

ADD, SUB and CMP retain the original operands as a lazy source for CF, PF, AF, ZF, SF
and OF. `FlagState` distinguishes stored CPU records from local flag sources.
A typed `FlagSource` retains either arithmetic operands or a logical result.
Constructing an `ArithmeticSource` builds value expressions; calling
`set_arithmetic_flags` retains that source as the new architectural flags.
A same-block condition uses only the expressions it needs; CMP and SUB conditions
can compare the original operands directly. Publication writes the zero-extended
operands to CPU dwords 4 and 8, then the kind byte at 0. SUB kinds 1/5/9 and ADD
kinds 2/6/10 denote byte/word/dword operands. AND, OR, XOR and TEST retain only
the logical result through `set_logic_flags`. Their records use kinds 3/7/11 with
the zero-extended result at offset 4; offset 8 is unused and remains untouched.
The flag owner retains only the current source and writes its record at publication;
replacing a source does not schedule or repair individual field writes. Logic clears
CF/OF and uses zero for architecturally undefined AF.
A nonzero kind owns all six status flags, so concrete flag bytes may be stale.
Kind 0 instead reads CF/PF/AF/ZF/SF/OF from bytes 12–17, each containing 0 or 1.
Other kind values trap when read. A stored direct query selects its exact record
kind before reading its typed inputs. Subtraction relations compare the original
operands; logical zero/nonzero queries compare only the result. Other queries use
shared readonly condition readers. Inverse
conditions share a reader and cached result. Readers are created only when needed;
querying a condition preserves the stored representation.
MOV and SETcc preserve flags, and these instructions leave non-status flag bytes
untouched. A faulting operand access preserves the previous instruction's flags.

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
The opcode field contains the first byte after any `66` prefixes, including `0F`
for an extended opcode. This reports
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
body. An error or missing completion discards the function and resources added
by its callback, and restores earlier forward declarations it completed. Discard
handles created by a failed callback. Separate `declare` and `define` remain
available for forward references and recursion. An open body can create a needed
helper through `body.program()`; the active function cannot be reopened.

Expressions support wrapping addition and subtraction, bitwise `and`/`or`/`xor`,
logical-width `popcnt`, `shl`, and `eq`/`ne` predicates that return `Val<I1>`. The borrowed `unsigned()` view
provides `shr`, `lt`, `ge` and zero extension, for example
`byte.unsigned().extend::<I32>().shl(8)`. `truncate::<I8>()` retains the low eight
bits. The borrowed `signed()` view provides `lt`/`ge` comparisons and sign extension, such as
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

A branch can load, store, contain nested `if_` calls, return, tail-call or trap. Ending
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

Use `switch` to execute one case selected by an integer value. The callback receives
`Some(key)` for a listed case and `None` for the default. Case keys must be unique
and fit the selector's logical type. Each case can fall through, return, tail-call
or trap; falling through continues after the switch.

```rust
body.switch(&opcode, &[0x28, 0x85], |arm, key| match key {
    Some(0x28) => arm.return_(1),
    Some(0x85) => arm.return_(2),
    _ => arm.return_(0),
})?;
```

`switch_value::<I32, _>` instead returns a value. Every case, including the default,
must yield, return, tail-call or trap, and at least one must yield. Child values obey
the same scope rules as conditional arms. Dense case ranges use a Wasm branch
table; sparse ranges use balanced comparisons without allocating a large table.
Calculations using only parameters and constants can be captured separately in
each arm that uses them. Nested branches within one arm share its capture. Values
used after the join retain their common capture. Memory reads, call results, joined
values and expressions depending on them keep their snapshot rules.

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
return, tail-call or trap, and at least one arm must yield. Arm effects remain ordered,
while an unused yielded expression and its possible traps can disappear. A join
preserves raw intermediate bits; narrow values are normalized when an operation
or function boundary needs their logical low bits. Construction errors discard
both arms and leave the parent usable.

`body.trap()` consumes its builder and ends that execution path with a WebAssembly
trap, regardless of the function result type.

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

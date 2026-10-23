# wasm-bindings

Portable Rust/Wasm value layouts, plus direct browser calls and callbacks for
functions supplied at runtime. The crate has no dependency on Relocade's compiler,
x86 types, or execution backend. Its codecs work on native targets too.

## Define values once

`WasmAbi` is re-exported from the `relocade-derive` procedural macro crate.
It generates the logical layout and Rust conversions for an owned struct or enum.

```rust
use relocade_wasm_bindings::{WasmAbi, lower_flat, lift_flat};

#[derive(WasmAbi, Debug, PartialEq)]
struct Position { x: u32, y: u16 }

#[derive(WasmAbi, Debug, PartialEq)]
enum Reply { Missing, Found(Position) }

let flat = lower_flat::<Reply>(Reply::Found(Position { x: 42, y: 7 }));
assert_eq!(flat, (1_i32, 42_i32, 7_i32));
assert_eq!(lift_flat::<Reply>(flat), Some(Reply::Found(Position { x: 42, y: 7 })));
```

Struct fields flatten in declaration order. Enums use a declaration-order `i32`
tag followed by overlapping payload slots. Each position uses the widest logical
slot needed by any variant; inactive slots are zero when encoding and ignored
when decoding. Float/integer overlap preserves float bits in an integer carrier.
Rust `repr` attributes and explicit discriminants do not change these ABI tags.
This is a function-call ABI, not a persistent serialization or Rust memory layout.

Supported values include signed and unsigned integers through 64 bits, `bool`,
`f32`, `f64`, tuples, fixed arrays, and derived aggregates. Narrow integers cross
Wasm boundaries as zero-extended logical bits; `bool` decodes only `0` and `1`.
Unknown enum tags fail conversion. Tuples and aggregate fields support up to
twelve components, and array codecs support lengths zero through twelve.
Browser calls support up to twelve flattened parameter slots and twelve result
slots, independently of how fields are nested.

## Browser functions and callbacks

`Function<P, R>` binds an existing Wasm function. `P` describes its arguments and
`R` its result. Calls accept and return ordinary Rust values, using the derived
codecs internally. `callback::<P, R>` turns a Rust closure into an actual Wasm
function that can be imported or stored in a `funcref` table.

```rust,no_run
# #[cfg(target_arch = "wasm32")]
# fn example(target: &js_sys::Function) -> relocade_wasm_bindings::Result<()> {
use relocade_wasm_bindings::{WasmAbi, Function, callback};

#[derive(WasmAbi)]
struct Request { key: u32, enabled: bool }
#[derive(WasmAbi)]
enum Reply { Missing, Found(u64) }

let function = Function::<(Request,), Reply>::new(target)?;
let reply = function.call((Request { key: 7, enabled: true },));
let host = callback::<(Request,), Reply>(|(request,)| {
    if request.enabled { Reply::Found(u64::from(request.key)) } else { Reply::Missing }
})?;
# Ok(())
# }
```

`Function::from_export(instance, name)` resolves an export directly.
`as_function()` exposes the original function, retaining its real Wasm signature,
including multiple results. Each handle owns its Rust table entry. Engine
references keep callbacks alive after Rust handles are dropped. Nested calls use
separate return areas on Rust's stack.

Link the wasm32 application with these flags:

```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "link-arg=--growable-table", "-C", "link-arg=--export=__stack_pointer"]
```

Both shared and unshared Rust memories are supported. Adapter compilation,
linking, and export resolution return errors. Execution traps and invalid ABI
values are fatal programming errors, not recoverable function results.

## Why not just wasm-bindgen?

We still use `wasm-bindgen` and `js-sys` for browser interop, linking, and callback
ownership. Its usual workflow postprocesses a compiled Rust module into a
JavaScript-facing API, and exported Rust structs become objects with generated
accessors. [wasm-bindgen's design](https://wasm-bindgen.github.io/wasm-bindgen/contributing/design/index.html)
and [struct bindings](https://wasm-bindgen.github.io/wasm-bindgen/contributing/design/exporting-rust-struct.html)
describe that contract.

Relocade also supplies new Wasm functions at runtime, after the application's
`wasm-bindgen` processing has finished. Calling an arbitrary export through
`js_sys::Function::call` or `apply` crosses JavaScript and converts values at the
boundary. Generic argument lists and multiple results also require JavaScript
arrays. That work matters for frequent host/guest calls, even when the function
itself is small.

A dynamic export exposes a raw Wasm signature, not a schema for a Rust struct or
a payload-carrying enum. `wasm-bindgen` supports richer types through generated
bindings, but those bindings do not automatically exist for a newly supplied
function. Someone still has to define field order, enum tags, payload storage,
and conversion of the arguments and results.

This crate derives that contract with `WasmAbi` and generates a small adapter when
binding a physical signature. Rust generics provide typed call shims, and adapter
modules are cached. Calls then pass scalars directly through Wasm, without a
JavaScript call bridge, argument array, or result conversion. This preserves
integer and floating-point bits, multiple results, and callbacks usable as real
`funcref` entries. The surrounding application's JavaScript API continues using
`wasm-bindgen`.

## Native integration

`AbiType` declares a logical shape. `AbiValue` adds its Rust codec. `WasmRepr`
determines the scalar slots, and `lower_flat` / `lift_flat` convert between Rust
values and the physical tuples accepted by native engines. `FunctionSignature`
decomposes Rust `fn(...) -> ...` types without depending on a compiler.

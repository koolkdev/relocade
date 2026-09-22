//! Explicit atomic effects, native widths and shared instances.

#[path = "atomics/ordering.rs"]
mod ordering;

use std::{path::Path, sync::Barrier};

use wasm86_compiler::{MemoryInt, Type, I16, I32, I64, I8};
use wasm86_test_support::{Module, SharedBytes};

use crate::{
    fixture::Fixture,
    wasm::{Input, MemoryBytes, Observation, TestModule, Value},
};

fn operations<T: MemoryInt>() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.shared_memory("memory", &[0x33; 24]);
    fixture.function(&[], &[T::TYPE; 10], |mut body| {
        body.atomic::<T>(memory, 8, 0)?.store(-1)?;
        let add = body.atomic::<T>(memory, 8, 0)?.add(2)?;
        let sub = body.atomic::<T>(memory, 7, 1)?.sub(3)?;
        let and = body.atomic::<T>(memory, 8, 0)?.and(15)?;
        let or = body.atomic::<T>(memory, 8, 0)?.or(128)?;
        let xor = body.atomic::<T>(memory, 8, 0)?.xor(255)?;
        let exchange = body.atomic::<T>(memory, 8, 0)?.exchange(85)?;
        let mismatch = body.atomic::<T>(memory, 8, 0)?.compare_exchange(34, 187)?;
        let equal = body.atomic::<T>(memory, 8, 0)?.compare_exchange(85, 170)?;
        let load = body.atomic::<T>(memory, 8, 0)?.load()?;
        // This effect must survive even though its returned value is discarded.
        body.atomic::<T>(memory, 8, 0)?.add(1)?;
        body.atomic_fence();
        let final_value = body.atomic::<T>(memory, 8, 0)?.load()?;
        body.return_([
            add,
            sub,
            and,
            or,
            xor,
            exchange,
            mismatch,
            equal,
            load,
            final_value,
        ])
    })
}

fn check_operations(v8: bool) {
    for (width, module) in [
        (1, operations::<I8>()),
        (2, operations::<I16>()),
        (4, operations::<I32>()),
        (8, operations::<I64>()),
    ] {
        let mask = match width {
            1 => 255,
            2 => 65535,
            4 => u32::MAX as u64,
            _ => u64::MAX,
        };
        let values = [mask, 1, mask - 1, 14, 142, 113, 85, 85, 170, 171].map(|value| {
            if width == 8 {
                Value::I64(value as i64)
            } else {
                Value::I32(value as i32)
            }
        });
        let mut expected = vec![0x33; 24];
        expected[8..8 + width].copy_from_slice(&171u64.to_le_bytes()[..width]);
        if v8 {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &[])
                        .with_memories(&[MemoryBytes::new("memory", &[0x33; 24])])
                ),
                Observation::returned(&values)
                    .with_memories(&[MemoryBytes::new("memory", &expected)])
            );
        } else {
            let mut instance = module.instantiate();
            assert_eq!(instance.call_values("run", &[]).unwrap(), values);
            assert_eq!(&instance.memory("memory")[..24], expected);
        }
    }
}

#[test]
fn native_operations_preserve_widths_results_and_unused_effects() {
    check_operations(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_operations_preserve_widths_results_and_unused_effects() {
    check_operations(true);
}

fn counter() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.shared_memory("counter", &[0; 4]);
    fixture.function(&[], &[Type::I32], |mut body| {
        let old = body.atomic::<I32>(memory, 0, 0)?.add(1)?;
        body.return_(old)
    })
}

#[test]
fn independent_instances_share_one_atomic_modification_order() {
    let module = counter();
    let shared = SharedBytes::new(1, 1);
    let ready = Barrier::new(4);
    let mut observed = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut instance =
                        module.instantiate_with_shared(&[("counter", shared.clone())]);
                    ready.wait();
                    (0..1024)
                        .map(|_| instance.call::<u32>(()).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observed.sort_unstable();
    assert_eq!(observed, (0..4096).collect::<Vec<_>>());
    assert_eq!(shared.read(0, 4), 4096u32.to_le_bytes());
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_independent_instances_share_one_atomic_modification_order() {
    let fixture = counter();
    let module = Module::new(fixture.bytes());
    let mut observed: Vec<u32> = module.run_v8(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/suites/atomics/workers.mjs"),
        &(),
    );
    observed.sort_unstable();
    assert_eq!(observed, (0..4096).collect::<Vec<_>>());
}

fn check_traps(v8: bool) {
    let mut fixture = Fixture::new();
    let memory = fixture.shared_memory("memory", &[0x55; 16]);
    let module = fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let address = body.parameter::<I32>(0)?;
        let previous = body.atomic::<I32>(memory, address, 0)?.exchange(0)?;
        body.return_(previous)
    });
    for address in [1, 65536] {
        if v8 {
            let observed = module.run_v8(
                &Input::call("run", &[Value::I32(address)])
                    .with_memories(&[MemoryBytes::new("memory", &[0x55; 16])]),
            );
            assert_eq!(observed.outcome, wasm86_test_support::Outcome::Trap);
            assert_eq!(observed.memories, [MemoryBytes::new("memory", &[0x55; 16])]);
        } else {
            let mut instance = module.instantiate();
            let trap = instance.call::<u32>(address).unwrap_err();
            assert_eq!(
                trap,
                if address == 1 {
                    wasmtime::Trap::HeapMisaligned
                } else {
                    wasmtime::Trap::MemoryOutOfBounds
                }
            );
            assert_eq!(&instance.memory("memory")[..16], &[0x55; 16]);
        }
    }
}

#[test]
fn atomic_alignment_and_bounds_trap_before_modification() {
    check_traps(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_atomic_alignment_and_bounds_trap_before_modification() {
    check_traps(true);
}

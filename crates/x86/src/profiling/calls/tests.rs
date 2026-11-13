use super::*;
use relocade_compiler::Program;
use std::collections::{BTreeMap, BTreeSet};
use wasmtime::{Linker, Memory, MemoryType, Store, TypedFunc};

struct Recorder {
    store: Store<()>,
    memory: Memory,
    record: TypedFunc<u32, u32>,
}

impl Recorder {
    fn new() -> Self {
        let mut program = Program::new();
        let recorder = CallProfile::declare_recorder(&mut program);
        program.export("record", recorder).unwrap();
        let module = relocade_test_support::Module::new(&program.compile().unwrap());
        let engine = relocade_test_support::engine();
        let mut store = Store::new(engine, ());
        let memory = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let mut linker = Linker::new(engine);
        linker
            .define(&store, "relocade", "callProfile", memory)
            .unwrap();
        let instance = linker.instantiate(&mut store, module.wasmtime()).unwrap();
        let record = instance.get_typed_func(&mut store, "record").unwrap();
        Self {
            store,
            memory,
            record,
        }
    }

    fn record(&mut self, target: u32) -> bool {
        self.record.call(&mut self.store, target).unwrap() != 0
    }

    fn drain(&mut self) -> BTreeMap<u32, u64> {
        let counts = CallProfile::read(|offset| {
            let mut bytes = [0; 4];
            self.memory
                .read(&self.store, offset as usize, &mut bytes)
                .unwrap();
            u32::from_le_bytes(bytes)
        });
        let mut result = BTreeMap::new();
        for entry in counts.iter() {
            *result.entry(entry.target).or_default() += u64::from(entry.count);
        }
        counts.clear(|offset, value| {
            self.memory
                .write(&mut self.store, offset as usize, &value.to_le_bytes())
                .unwrap();
        });
        result
    }
}

fn bucket(target: u32) -> u32 {
    let product = target.wrapping_mul(0x9e37_79b1);
    (product ^ (product >> 16)) & (SLOTS - 1)
}

#[test]
fn repeated_targets_including_zero_survive_drain_and_reuse() {
    let mut recorder = Recorder::new();
    for target in [0, u32::MAX, 0, 42, 42, 42] {
        assert!(recorder.record(target));
    }
    assert_eq!(
        recorder.drain(),
        BTreeMap::from([(0, 2), (42, 3), (u32::MAX, 1)])
    );
    assert!(recorder.drain().is_empty());
    assert!(recorder.record(42));
    assert_eq!(recorder.drain(), BTreeMap::from([(42, 1)]));
}

#[test]
fn probe_exhaustion_retains_the_triggering_call() {
    let targets: Vec<_> = (0..)
        .filter(|&target| bucket(target) == 0)
        .take(9)
        .collect();
    let mut recorder = Recorder::new();
    let mut merged = BTreeMap::<u32, u64>::new();
    let mut flushes = 0;
    for &target in targets.iter().cycle().take(90) {
        if !recorder.record(target) {
            flushes += 1;
            for (key, count) in recorder.drain() {
                *merged.entry(key).or_default() += count;
            }
        }
    }
    for (key, count) in recorder.drain() {
        *merged.entry(key).or_default() += count;
    }
    assert_eq!(flushes, 10);
    assert_eq!(merged, targets.into_iter().map(|key| (key, 10)).collect());
}

#[test]
fn capacity_limits_new_targets_but_allows_existing_targets() {
    let mut buckets = BTreeSet::new();
    let targets: Vec<_> = (0..)
        .filter(|&target| buckets.insert(bucket(target)))
        .take(513)
        .collect();
    let mut recorder = Recorder::new();
    for &target in &targets[..512] {
        assert!(recorder.record(target));
    }
    assert!(recorder.record(targets[0]));
    assert!(!recorder.record(targets[512]));
    let counts = recorder.drain();
    assert_eq!(counts.len(), 513);
    assert_eq!(counts.values().sum::<u64>(), 514);
    assert_eq!(counts[&targets[0]], 2);
    assert_eq!(counts[&targets[512]], 1);
}

#[test]
fn count_exhaustion_does_not_wrap_to_an_empty_slot() {
    let mut recorder = Recorder::new();
    assert!(recorder.record(0));
    recorder
        .memory
        .write(
            &mut recorder.store,
            COUNTS as usize,
            &u32::MAX.to_le_bytes(),
        )
        .unwrap();
    assert!(!recorder.record(0));
    assert_eq!(
        recorder.drain(),
        BTreeMap::from([(0, u64::from(u32::MAX) + 1)])
    );
}

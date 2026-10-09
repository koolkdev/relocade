use super::*;
use crate::{
    body::{Value, ValueDefinition},
    memory::{Mem, MemoryAccess},
    Func, Type,
};

fn parameter(graph: &mut FunctionGraph) -> usize {
    let component = graph.blocks[graph.entry.0].parameters.len();
    let id = graph.values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component,
        },
    });
    graph.blocks[graph.entry.0].parameters.push(id);
    id
}

fn access(memory: Mem, offset: u32) -> MemoryAccess {
    MemoryAccess {
        memory,
        offset,
        bytes: 4,
    }
}

#[test]
fn scalar_and_bulk_writes_share_relative_alias_precision() {
    let mut graph = FunctionGraph::new();
    let base = parameter(&mut graph);
    let other_base = parameter(&mut graph);
    let four = graph.values.literal(Type::I32, 4);
    let value = graph.values.literal(Type::I32, 7);
    let writers = [
        Operation::store(access(Mem(0), 0), base, value),
        Operation::memory_fill(Mem(0), base, value, four),
        Operation::memory_copy(Mem(0), Mem(1), base, other_base, four),
    ];
    for writer in writers {
        for (offset, conflict) in [(0, true), (2, true), (4, false)] {
            let reader = Operation::load(access(Mem(0), offset), base);
            assert_eq!(blocks_read(&writer, &reader, &graph, &[]), conflict);
        }
        let distinct_memory = Operation::load(access(Mem(1), 0), base);
        assert!(!blocks_read(&writer, &distinct_memory, &graph, &[]));
        let unknown_alias = Operation::load(access(Mem(0), 8), other_base);
        assert!(blocks_read(&writer, &unknown_alias, &graph, &[]));
    }
}

#[test]
fn zero_lengths_and_unknown_lengths_have_distinct_dependencies() {
    let mut graph = FunctionGraph::new();
    let destination = parameter(&mut graph);
    let length = parameter(&mut graph);
    let zero = graph.values.literal(Type::I32, 0);
    let reader = Operation::load(access(Mem(0), 0), zero);
    for (bytes, conflict) in [(zero, false), (length, true)] {
        let writer = Operation::memory_fill(Mem(0), destination, zero, bytes);
        assert_eq!(blocks_read(&writer, &reader, &graph, &[]), conflict);
        assert!(observable(&writer, &[]));

        let mut writes = Vec::new();
        include(
            &mut writes,
            [MemoryRange::from_span(Mem(0), destination, bytes, &graph)],
        );
        let summaries = [Effects::Known {
            reads: Vec::new(),
            writes,
            synchronizes: false,
        }];
        let call = Operation::call(Func(0), Vec::new());
        assert_eq!(blocks_read(&call, &reader, &graph, &summaries), conflict);
        assert!(observable(&call, &summaries));
    }
}

#[test]
fn summaries_discard_local_addresses_but_keep_absolute_ranges() {
    let mut graph = FunctionGraph::new();
    let base = parameter(&mut graph);
    let absolute = graph.values.literal(Type::I32, 12);
    let mut writes = Vec::new();
    include(
        &mut writes,
        [MemoryRange::from_location(
            access(Mem(0), 8).at(base),
            &graph,
        )],
    );
    let summaries = [Effects::Known {
        reads: Vec::new(),
        writes,
        synchronizes: false,
    }];
    // A caller's value with the same numerical ID is unrelated to the callee's.
    let reader = Operation::load(access(Mem(0), 0), base);
    let writer = Operation::call(Func(0), Vec::new());
    assert!(blocks_read(&writer, &reader, &graph, &summaries));

    let mut writes = Vec::new();
    include(
        &mut writes,
        [MemoryRange::from_location(
            access(Mem(0), 0).at(absolute),
            &graph,
        )],
    );
    let summaries = [Effects::Known {
        reads: Vec::new(),
        writes,
        synchronizes: false,
    }];
    let adjacent = Operation::load(access(Mem(0), 4), absolute);
    assert!(!blocks_read(&writer, &adjacent, &graph, &summaries));
    let overlapping = Operation::load(access(Mem(0), 2), absolute);
    assert!(blocks_read(&writer, &overlapping, &graph, &summaries));
}

#[test]
fn synchronization_and_unknown_calls_block_memory_reads() {
    let mut graph = FunctionGraph::new();
    let address = parameter(&mut graph);
    let load = Operation::load(access(Mem(0), 0), address);
    let summaries = [
        Effects::Unknown,
        Effects::Known {
            reads: Vec::new(),
            writes: Vec::new(),
            synchronizes: false,
        },
        Effects::Known {
            reads: Vec::new(),
            writes: Vec::new(),
            synchronizes: true,
        },
    ];
    let pure_call = Operation::call(Func(1), Vec::new());
    for barrier in [
        Operation::fence(),
        Operation::atomic_load(access(Mem(1), 0), address),
        Operation::call(Func(0), Vec::new()),
        Operation::call(Func(2), Vec::new()),
    ] {
        assert!(blocks_read(&barrier, &load, &graph, &summaries));
        assert!(!blocks_read(&barrier, &pure_call, &graph, &summaries));
    }
}

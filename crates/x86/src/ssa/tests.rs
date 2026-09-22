use super::{Environment, Location};
use wasm86_compiler::{FunctionBuilder, MemoryImport, Program, Signature, Type, Val, I16, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[derive(Debug, Eq, PartialEq)]
enum Access {
    Load(u64),
    LoadByte(u64),
    LoadWord(u64),
    Store(u64, Option<i32>),
    StoreByte(u64, Option<i32>),
    StoreWord(u64, Option<i32>),
}

fn accesses(
    build: impl FnOnce(&mut FunctionBuilder<'_>, &mut Environment) -> Val<I32>,
) -> Vec<Access> {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    let mut body = program.define(function).unwrap();
    let mut state = Environment::new(memory);
    let result = build(&mut body, &mut state);
    body.return_(result).unwrap();
    program.export("run", function).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut accesses = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(code) = payload.unwrap() {
            let mut previous_constant = None;
            for operation in code.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                match operation {
                    Operator::I32Load8U { memarg } => {
                        accesses.push(Access::LoadByte(memarg.offset))
                    }
                    Operator::I32Load16U { memarg } => {
                        accesses.push(Access::LoadWord(memarg.offset))
                    }
                    Operator::I32Store8 { memarg } => {
                        accesses.push(Access::StoreByte(memarg.offset, previous_constant));
                    }
                    Operator::I32Store16 { memarg } => {
                        accesses.push(Access::StoreWord(memarg.offset, previous_constant));
                    }
                    Operator::I32Load { memarg } => accesses.push(Access::Load(memarg.offset)),
                    Operator::I32Store { memarg } => {
                        accesses.push(Access::Store(memarg.offset, previous_constant));
                    }
                    _ => {}
                }
                previous_constant = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
            }
        }
    }
    accesses
}

#[test]
fn repeated_reads_reuse_the_old_value_after_a_new_definition() {
    let emitted = accesses(|body, state| {
        let first = state.read(body, Location::<I32>::new(0)).unwrap();
        let second = state.read(body, Location::<I32>::new(0)).unwrap();
        state.define(body, Location::<I32>::new(0), 9).unwrap();
        let current = state.read(body, Location::<I32>::new(0)).unwrap();
        state.publish(body).unwrap();
        first.add(second).add(current)
    });
    assert_eq!(emitted, [Access::Load(0), Access::Store(0, Some(9))]);
}

#[test]
fn defining_the_value_already_in_backing_needs_no_store() {
    let emitted = accesses(|body, state| {
        let value = state.read(body, Location::<I32>::new(0)).unwrap();
        state.define(body, Location::<I32>::new(0), &value).unwrap();
        state.publish(body).unwrap();
        value
    });
    assert_eq!(emitted, [Access::Load(0)]);
}

#[test]
fn first_writes_order_publication_independently_of_reads_and_overwrites() {
    let emitted = accesses(|body, state| {
        let old = state.read(body, Location::<I32>::new(8)).unwrap();
        state.define(body, Location::<I32>::new(0), 7).unwrap();
        state.define(body, Location::<I32>::new(8), 9).unwrap();
        state.define(body, Location::<I32>::new(0), 11).unwrap();
        state.publish(body).unwrap();
        old
    });
    assert_eq!(
        emitted,
        [
            Access::Load(8),
            Access::Store(0, Some(11)),
            Access::Store(8, Some(9))
        ]
    );
}

#[test]
fn computed_accesses_synchronize_and_invalidate_only_their_declared_range() {
    let emitted = accesses(|body, state| {
        let address = body.parameter::<I32>(0).unwrap().and(1).shl(2);
        state.define(body, Location::<I32>::new(16), 7).unwrap();
        state.define(body, Location::<I32>::new(24), 9).unwrap();
        let indexed = Location::<I32>::indexed(16, 8, address);
        let before = state.read(body, indexed.clone()).unwrap();
        state.define(body, indexed, 11).unwrap();
        let after = state.read(body, Location::<I32>::new(16)).unwrap();
        let disjoint = state.read(body, Location::<I32>::new(24)).unwrap();
        state.publish(body).unwrap();
        before.add(after).add(disjoint)
    });
    assert_eq!(
        emitted,
        [
            Access::Store(16, Some(7)),
            Access::Load(16),
            Access::Store(16, Some(11)),
            Access::Store(24, Some(9)),
            Access::Load(16),
        ]
    );
}

#[test]
fn partial_accesses_preserve_other_bytes_and_held_values() {
    let emitted = accesses(|body, state| {
        state
            .define(body, Location::<I32>::new(0), 0x1122_3344)
            .unwrap();
        let old_word = state.read(body, Location::<I16>::new(0)).unwrap();
        state.define(body, Location::<I8>::new(1), 0xaa).unwrap();
        let patched_word = state.read(body, Location::<I16>::new(0)).unwrap();
        state.define(body, Location::<I16>::new(0), 0xbbcc).unwrap();
        let current = state.read(body, Location::<I32>::new(0)).unwrap();
        state.publish(body).unwrap();
        old_word
            .unsigned()
            .extend::<I32>()
            .add(patched_word.unsigned().extend::<I32>())
            .add(current)
    });
    assert_eq!(
        emitted,
        [
            Access::Store(0, Some(0x1122_3344)),
            Access::LoadWord(0),
            Access::StoreByte(1, Some(0xaa)),
            Access::LoadWord(0),
            Access::StoreWord(0, Some(0xbbcc)),
            Access::Load(0),
        ]
    );
}

#[test]
fn covering_definitions_replace_pending_partial_writes() {
    let emitted = accesses(|body, state| {
        state.define(body, Location::<I8>::new(0), 0x11).unwrap();
        state.define(body, Location::<I8>::new(1), 0x22).unwrap();
        state.define(body, Location::<I16>::new(0), 0x3344).unwrap();
        state
            .define(body, Location::<I32>::new(0), 0x5566_7788)
            .unwrap();
        state.define(body, Location::<I16>::new(8), 0x99aa).unwrap();
        state.define(body, Location::<I8>::new(10), 0xbb).unwrap();
        state.publish(body).unwrap();
        body.parameter::<I32>(0).unwrap()
    });
    assert_eq!(
        emitted,
        [
            Access::Store(0, Some(0x5566_7788)),
            Access::StoreWord(8, Some(0x99aa)),
            Access::StoreByte(10, Some(0xbb)),
        ]
    );
}

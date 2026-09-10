use wasm86_x86::{compile_interpreter_step, CompiledModule};

mod support;

#[path = "suites/arithmetic_flags.rs"]
mod arithmetic_flags;
#[path = "suites/binary_decoding.rs"]
mod binary_decoding;
#[path = "suites/binary_operands.rs"]
mod binary_operands;
#[path = "suites/byte_moves.rs"]
mod byte_moves;
#[path = "suites/carry_arithmetic.rs"]
mod carry_arithmetic;
#[path = "suites/count_publication.rs"]
mod count_publication;
#[path = "suites/extending_moves.rs"]
mod extending_moves;
#[path = "suites/immediate_and_absolute_moves.rs"]
mod immediate_and_absolute_moves;
#[path = "suites/instruction_prefixes.rs"]
mod instruction_prefixes;
#[path = "suites/interpreter_steps.rs"]
mod interpreter_steps;
#[path = "suites/logical_flags.rs"]
mod logical_flags;
#[path = "suites/memory_moves.rs"]
mod memory_moves;
#[path = "suites/mov_blocks.rs"]
mod mov_blocks;
#[path = "suites/operand_fetch.rs"]
mod operand_fetch;
#[path = "suites/relative_branches.rs"]
mod relative_branches;
#[path = "suites/stack_operations.rs"]
mod stack_operations;
#[path = "suites/unary_operations.rs"]
mod unary_operations;
#[path = "suites/word_moves.rs"]
mod word_moves;

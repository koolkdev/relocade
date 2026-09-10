#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/wasm.rs"]
mod wasm;

#[path = "suites/bit_counts.rs"]
mod bit_counts;
#[path = "suites/blocks.rs"]
mod blocks;
#[path = "suites/branch_placement.rs"]
mod branch_placement;
#[path = "suites/computed_memory.rs"]
mod computed_memory;
#[path = "suites/conditional_values.rs"]
mod conditional_values;
#[path = "suites/control_flow.rs"]
mod control_flow;
#[path = "suites/execution.rs"]
mod execution;
#[path = "suites/function_bodies.rs"]
mod function_bodies;
#[path = "suites/generated_code.rs"]
mod generated_code;
#[path = "suites/integer_ops.rs"]
mod integer_ops;
#[path = "suites/memory.rs"]
mod memory;
#[path = "suites/no_result_functions.rs"]
mod no_result_functions;
#[path = "suites/ordinary_calls.rs"]
mod ordinary_calls;
#[path = "suites/signed_arithmetic.rs"]
mod signed_arithmetic;
#[path = "suites/switches.rs"]
mod switches;
#[path = "suites/tail_calls.rs"]
mod tail_calls;
#[path = "suites/value_selection.rs"]
mod value_selection;
#[path = "suites/zero_tests.rs"]
mod zero_tests;

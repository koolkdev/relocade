use super::{exception, unsupported, ExecutionExit};
use crate::exception::Exception;
use crate::test_step::{
    Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule,
};
use crate::{CompiledModule, CpuState};
use wasm86_compiler::{Program, Signature, Type, I32, I8};

fn exit_module() -> Vec<u8> {
    let mut program = Program::new();
    for name in [
        "divide_error",
        "bound_range_exceeded",
        "invalid_opcode",
        "segment_not_present",
        "stack_fault",
        "general_protection",
        "page_fault",
        "floating_point",
        "simd_floating_point",
        "unsupported",
    ] {
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32, Type::I32],
                    results: vec![Type::I64],
                },
                |body| {
                    let detail = body.parameter::<I32>(0)?;
                    let address = body.parameter::<I32>(1)?;
                    let fault = match name {
                        "divide_error" => Exception::DivideError,
                        "bound_range_exceeded" => Exception::BoundRangeExceeded,
                        "invalid_opcode" => Exception::InvalidOpcode,
                        "floating_point" => Exception::FloatingPoint,
                        "simd_floating_point" => Exception::SimdFloatingPoint,
                        "segment_not_present" => {
                            Exception::SegmentNotPresent { error_code: detail }
                        }
                        "stack_fault" => Exception::StackFault { error_code: detail },
                        "general_protection" => Exception::GeneralProtection { error_code: detail },
                        "page_fault" => Exception::PageFault {
                            linear_address: address,
                            error_code: detail,
                        },
                        "unsupported" => {
                            return unsupported(body, &address, detail.truncate::<I8>())
                        }
                        _ => unreachable!(),
                    };
                    exception(body, fault)
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    program.compile().unwrap()
}

fn check_host_exit_words(engine: Engine) {
    let bytes = exit_module();
    let cpu = CpuState::filled(0xa5).to_bytes();
    for (name, detail, address, expected) in [
        (
            "divide_error",
            0x1234,
            0x89ab_cdef_u32,
            0x0001_0000_0000_0000_i64,
        ),
        ("segment_not_present", 0, 0x89ab_cdef, 0x0020_0000_0000_0000),
        ("invalid_opcode", 0xffff, 0x89ab_cdef, 0x0080_0000_0000_0000),
        (
            "bound_range_exceeded",
            0xffff,
            0x89ab_cdef,
            0x0040_0000_0000_0000,
        ),
        (
            "segment_not_present",
            0xfffc,
            0x89ab_cdef,
            0x0020_fffc_0000_0000,
        ),
        ("general_protection", 0, 0x89ab_cdef, 0x0002_0000_0000_0000),
        ("stack_fault", 0, 0x89ab_cdef, 0x0010_0000_0000_0000),
        ("stack_fault", 0xabcd, 0x89ab_cdef, 0x0010_abcd_0000_0000),
        (
            "general_protection",
            0xabcd,
            0x89ab_cdef,
            0x0002_abcd_0000_0000,
        ),
        ("page_fault", 0, 0x89ab_cdef, 0x0004_0000_89ab_cdef),
        ("page_fault", 0x13, 0xffff_ffff, 0x0004_0013_ffff_ffff),
        ("floating_point", 0xffff, 0x89ab_cdef, 0x0100_0000_0000_0000),
        (
            "simd_floating_point",
            0xffff,
            0x89ab_cdef,
            0x0200_0000_0000_0000,
        ),
        ("unsupported", 0xf3, 0x89ab_cdef, 0x0008_00f3_89ab_cdef),
    ] {
        let module = TestModule::new(&CompiledModule {
            execution_profile: None,
            bytes: bytes.clone(),
            entry: name.into(),
        });
        let input = Input {
            arguments: vec![Argument::I32(detail), Argument::I32(address as i32)],
            ..Input::new(&cpu)
        };
        assert_eq!(
            engine.observe(&module, &input, 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I64(expected)]),
                    snapshot: Snapshot {
                        cpu: cpu.to_vec(),
                        guest: None
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "{name}, detail {detail:#x}, address {address:#x}"
        );
    }
}

#[test]
fn host_exit_words_in_wasmtime() {
    check_host_exit_words(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn host_exit_words_in_v8() {
    check_host_exit_words(Engine::V8);
}

#[test]
fn decode_documented_guest_exit_words() {
    assert_eq!(
        ExecutionExit::from_bits(0x0400_0000_0000_0000),
        Some(ExecutionExit::BudgetExhausted),
    );
    assert_eq!(
        ExecutionExit::from_bits(0x0402_0000_0000_0000),
        Some(ExecutionExit::InstructionLimit)
    );
    assert_eq!(ExecutionExit::from_bits(0x0402_0000_0000_0001), None);
    for (bits, exception) in [
        (0x0001_0000_0000_0000, Exception::DivideError),
        (0x0040_0000_0000_0000, Exception::BoundRangeExceeded),
        (0x0080_0000_0000_0000, Exception::InvalidOpcode),
        (0x0100_0000_0000_0000, Exception::FloatingPoint),
        (0x0200_0000_0000_0000, Exception::SimdFloatingPoint),
        (
            0x0002_abcd_0000_0000,
            Exception::GeneralProtection { error_code: 0xabcd },
        ),
        (
            0x0010_0000_0000_0000,
            Exception::StackFault { error_code: 0 },
        ),
        (
            0x0020_fffc_0000_0000,
            Exception::SegmentNotPresent { error_code: 0xfffc },
        ),
        (
            0x0004_0013_ffff_ffff,
            Exception::PageFault {
                linear_address: u32::MAX,
                error_code: 0x13,
            },
        ),
    ] {
        assert_eq!(
            ExecutionExit::from_bits(bits),
            Some(ExecutionExit::Exception(exception))
        );
    }
    assert_eq!(
        ExecutionExit::from_bits(0x0008_00f3_89ab_cdef),
        Some(ExecutionExit::UnsupportedInstruction {
            eip: 0x89ab_cdef,
            opcode: 0xf3
        }),
    );
}

#[test]
fn host_values_and_reserved_exit_fields_are_not_guest_exits() {
    for bits in [
        0,
        1,
        u64::MAX,
        0x8000_0000_0000_0000,
        0x0003_0000_0000_0000,
        0x0001_0001_0000_0000,
        0x0040_0000_0000_0001,
        0x0080_0001_0000_0000,
        0x0100_0000_0000_0001,
        0x0200_0000_0000_0001,
        0x0200_0001_0000_0000,
        0x0400_0000_0000_0001,
        0x0400_0001_0000_0000,
        0x0002_0000_0000_0001,
        0x0010_0000_0000_0001,
        0x0020_0000_0000_0001,
        0x0008_0100_0000_0000,
    ] {
        assert_eq!(ExecutionExit::from_bits(bits), None, "{bits:#018x}");
    }
}

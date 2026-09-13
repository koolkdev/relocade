use super::{exception, unsupported};
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
        "general_protection",
        "page_fault",
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
                        "general_protection" => Exception::GeneralProtection { error_code: detail },
                        "page_fault" => Exception::PageFault {
                            linear_address: address,
                            error_code: detail,
                        },
                        "unsupported" => {
                            return unsupported(body, &address, &detail.truncate::<I8>())
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
        ("general_protection", 0, 0x89ab_cdef, 0x0002_0000_0000_0000),
        (
            "general_protection",
            0xabcd,
            0x89ab_cdef,
            0x0002_abcd_0000_0000,
        ),
        ("page_fault", 0, 0x89ab_cdef, 0x0004_0000_89ab_cdef),
        ("page_fault", 0x13, 0xffff_ffff, 0x0004_0013_ffff_ffff),
        ("unsupported", 0xf3, 0x89ab_cdef, 0x0008_00f3_89ab_cdef),
    ] {
        let module = TestModule::new(&CompiledModule {
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

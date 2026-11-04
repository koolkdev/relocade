use super::{exception, unsupported, ExecutionExit, ExecutionExitValues, HostExit, HostExitValues};
use crate::test_step::{returned, Argument, Event, Input, Observation, Snapshot, TestModule};
use crate::{CompiledModule, CpuState, Exception, ExceptionValues};
use relocade_compiler::{abi::Function, Program, I8};

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
        "interrupt_entered",
        "interrupt_inhibited",
        "interrupted",
        "step_complete",
        "interrupts_enabled",
        "halted",
        "host_return",
        "host_call",
        "host_resume",
    ] {
        Function::<fn(u32, u32) -> ExecutionExit>::new("test", name)
            .define(&mut program, move |body, (detail, address)| {
                let fault = match name {
                    "divide_error" => ExceptionValues::divide_error(),
                    "bound_range_exceeded" => ExceptionValues::bound_range_exceeded(),
                    "invalid_opcode" => ExceptionValues::invalid_opcode(),
                    "floating_point" => ExceptionValues::floating_point(),
                    "simd_floating_point" => ExceptionValues::simd_floating_point(),
                    "segment_not_present" => ExceptionValues::segment_not_present(detail),
                    "stack_fault" => ExceptionValues::stack_fault(detail),
                    "general_protection" => ExceptionValues::general_protection(detail),
                    "page_fault" => ExceptionValues::page_fault(address, detail),
                    "host_return" => {
                        return body.return_(ExecutionExitValues::host(HostExit::Return))
                    }
                    "host_call" => {
                        return body
                            .return_(ExecutionExitValues::host(HostExitValues::call(address)))
                    }
                    "host_resume" => {
                        return body.return_(ExecutionExitValues::host(HostExit::Resume))
                    }
                    "interrupt_entered" => {
                        return body.return_(ExecutionExitValues::interrupt_entered())
                    }
                    "interrupt_inhibited" => {
                        return body.return_(ExecutionExitValues::interrupt_inhibited())
                    }
                    "interrupted" => return body.return_(ExecutionExitValues::interrupted()),
                    "interrupts_enabled" => {
                        return body.return_(ExecutionExitValues::interrupts_enabled())
                    }
                    "step_complete" => return body.return_(ExecutionExitValues::step_complete()),
                    "halted" => return body.return_(ExecutionExitValues::halted()),
                    "unsupported" => return unsupported(body, &address, detail.truncate::<I8>()),
                    _ => unreachable!(),
                };
                exception(body, fault)
            })
            .unwrap();
    }
    program.compile().unwrap()
}

#[test]
fn generated_exits_preserve_payloads() {
    let bytes = exit_module();
    let cpu = CpuState::filled(0xa5).to_bytes();
    let detail = 0xf123_abcd_u32;
    let address = 0x89ab_cdef_u32;
    let mut cases = vec![
        ("divide_error", Exception::DivideError),
        ("bound_range_exceeded", Exception::BoundRangeExceeded),
        ("invalid_opcode", Exception::InvalidOpcode),
        (
            "segment_not_present",
            Exception::SegmentNotPresent { error_code: detail },
        ),
        ("stack_fault", Exception::StackFault { error_code: detail }),
        (
            "general_protection",
            Exception::GeneralProtection { error_code: detail },
        ),
        (
            "page_fault",
            Exception::PageFault {
                linear_address: address,
                error_code: detail,
            },
        ),
        ("floating_point", Exception::FloatingPoint),
        ("simd_floating_point", Exception::SimdFloatingPoint),
    ]
    .into_iter()
    .map(|(name, fault)| (name, ExecutionExit::Exception(fault)))
    .collect::<Vec<_>>();
    cases.extend([
        (
            "unsupported",
            ExecutionExit::UnsupportedInstruction {
                eip: address,
                opcode: detail as u8,
            },
        ),
        ("interrupt_entered", ExecutionExit::InterruptEntered),
        ("interrupt_inhibited", ExecutionExit::InterruptInhibited),
        ("interrupted", ExecutionExit::Interrupted),
        ("step_complete", ExecutionExit::StepComplete),
        ("interrupts_enabled", ExecutionExit::InterruptsEnabled),
        ("halted", ExecutionExit::Halted),
        ("host_return", ExecutionExit::Host(HostExit::Return)),
        ("host_resume", ExecutionExit::Host(HostExit::Resume)),
        (
            "host_call",
            ExecutionExit::Host(HostExit::Call {
                service_id: address,
            }),
        ),
    ]);
    for (name, expected) in cases {
        let module = TestModule::new(&CompiledModule {
            execution_target: None,
            bytes: bytes.clone(),
            entry: name.into(),
        });
        let input = Input {
            arguments: vec![Argument::I32(detail as i32), Argument::I32(address as i32)],
            ..Input::new(&cpu)
        };
        assert_eq!(
            module.observe(&input, 1),
            Observation {
                events: vec![Event::Return {
                    outcome: returned(expected),
                    snapshot: Snapshot {
                        cpu: cpu.to_vec(),
                        guest: None
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "{name}"
        );
    }
}

#[test]
fn execution_exits_share_four_wasm_values() {
    use relocade_wasm_bindings::{lift_flat, lower_flat};

    let address = 0x89ab_cdef;
    let error_code = 0xf123_abcd;
    let cases = [
        (ExecutionExit::Host(HostExit::Return), (0, 0, 0, 0)),
        (ExecutionExit::Host(HostExit::Resume), (0, 2, 0, 0)),
        (
            ExecutionExit::Host(HostExit::Call {
                service_id: address,
            }),
            (0, 1, address as i32, 0),
        ),
        (ExecutionExit::InterruptEntered, (3, 0, 0, 0)),
        (ExecutionExit::InterruptInhibited, (4, 0, 0, 0)),
        (ExecutionExit::Interrupted, (5, 0, 0, 0)),
        (ExecutionExit::StepComplete, (6, 0, 0, 0)),
        (ExecutionExit::InterruptsEnabled, (7, 0, 0, 0)),
        (ExecutionExit::Halted, (8, 0, 0, 0)),
        (
            ExecutionExit::Exception(Exception::PageFault {
                linear_address: address,
                error_code,
            }),
            (1, 6, address as i32, error_code as i32),
        ),
        (
            ExecutionExit::UnsupportedInstruction {
                eip: address,
                opcode: 0xff,
            },
            (2, address as i32, 255, 0),
        ),
    ];
    for (exit, flat) in cases {
        assert_eq!(lower_flat::<ExecutionExit>(exit), flat);
        assert_eq!(lift_flat::<ExecutionExit>(flat), Some(exit));
    }
}

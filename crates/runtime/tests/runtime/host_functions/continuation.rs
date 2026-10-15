use super::*;
use wasm86_runtime::{SliceOutcome, SliceStop};
use wasm86_x86::{Exception, ExecutionExit};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn budget_and_host_yields_resume_without_repeating_the_callback() {
    for action in [HostAction::Continue, HostAction::Yield] {
        let mut machine = machine(&[]).await;
        let calls = count_calls(&mut machine, 0x100, action).await;
        return_frame(&mut machine, 0x200);
        machine.write_memory(0x200, &[0xe9, 0xfd, 0xfe]).unwrap(); // JMP 0100
        assert_eq!(machine.run_slice(0).unwrap().work, 0);
        assert_eq!(calls.get(), 0);
        let stop = match action {
            HostAction::Continue => SliceStop::WorkLimit,
            HostAction::Yield => SliceStop::Exited(ExecutionExit::HostYield { address: 0x100 }),
        };
        assert_eq!(
            machine.run_slice(1).unwrap(),
            SliceOutcome {
                work: 1,
                completed: 0,
                stop
            }
        );
        assert_eq!(machine.cpu().eip, 0x100);
        // Updating the table must retain the suspended callable.
        install(&mut machine, 0x300, HostReturn::Interrupt, |_| {
            HostAction::Continue
        })
        .await;
        assert_eq!(machine.run_slice(1).unwrap().completed, 1);
        assert_eq!((calls.get(), machine.cpu().eip), (1, 0x200));
        assert_eq!(machine.run_slice(1).unwrap().stop, SliceStop::WorkLimit); // JMP back
        machine.run_slice(1).unwrap();
        assert_eq!(calls.get(), 2);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn redirected_host_entries_share_the_work_budget() {
    let mut machine = machine(&[]).await;
    let mut functions = HostFunctions::new();
    for (entry, next) in [(0x100, 0x200), (0x200, 0x100)] {
        functions.insert(entry, HostReturn::Interrupt, move |host| {
            let mut cpu = host.cpu();
            cpu.eip = next;
            host.set_cpu(cpu).unwrap();
            HostAction::Continue
        });
    }
    machine.install_host_functions(functions).await.unwrap();
    assert_eq!(
        machine.run_slice(5).unwrap(),
        SliceOutcome {
            work: 5,
            completed: 0,
            stop: SliceStop::WorkLimit,
        }
    );
    assert_eq!(machine.cpu().eip, 0x200);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn explicit_restart_at_same_address_abandons_the_suspended_callable() {
    let mut machine = machine(&[0x40]).await;
    let calls = count_calls(&mut machine, 0x100, HostAction::Yield).await;
    return_frame(&mut machine, 0x200);
    machine.run_slice(1).unwrap();
    let mut cpu = machine.cpu();
    cpu.registers.eax = 10;
    machine.set_cpu(cpu).unwrap();
    machine.restart_entry();
    assert_eq!(
        machine.run_slice(1).unwrap().stop,
        SliceStop::Exited(ExecutionExit::HostYield { address: 0x100 })
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(machine.run_slice(1).unwrap().stop, SliceStop::WorkLimit);
    assert_eq!((calls.get(), machine.cpu().registers.eax), (2, 10));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn repairing_a_faulting_compiled_return_does_not_replay_host_effects() {
    let mut machine = machine(&[0xcf]).await;
    let calls = count_calls(&mut machine, 0x100, HostAction::Continue).await;
    let mut cpu = machine.cpu();
    cpu.registers.esp = 0xfffc;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice(2).unwrap().stop,
        SliceStop::Exited(ExecutionExit::Exception(Exception::StackFault {
            error_code: 0
        }))
    );
    let mut cpu = machine.cpu();
    cpu.registers.esp = 0xfe00;
    machine.set_cpu(cpu).unwrap();
    machine.write_memory(0xfe00, &[0, 2, 0, 0, 2, 0]).unwrap();
    assert_eq!(machine.run_slice(1).unwrap().stop, SliceStop::WorkLimit);
    assert_eq!(calls.get(), 1);
    assert_eq!(
        (machine.cpu().eip, machine.cpu().registers.esp),
        (0x200, 0xfe06)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn replacing_or_removing_a_suspended_service_preserves_its_return() {
    for replace in [false, true] {
        let mut machine = machine(&[0x40]).await;
        let old = count_calls(&mut machine, 0x100, HostAction::Yield).await;
        return_frame(&mut machine, 0x200);
        machine.run_slice(1).unwrap();
        let new = if replace {
            Some(count_calls(&mut machine, 0x100, HostAction::Continue).await)
        } else {
            assert!(machine.remove_entry(0x100).unwrap());
            None
        };
        assert_eq!(machine.run_slice(1).unwrap().stop, SliceStop::WorkLimit);
        assert_eq!((old.get(), machine.cpu().eip), (1, 0x200));
        assert!(new.as_ref().is_none_or(|calls| calls.get() == 0));
        let mut cpu = machine.cpu();
        cpu.eip = 0x100;
        machine.set_cpu(cpu).unwrap();
        return_frame(&mut machine, 0x200);
        machine.run_slice(if replace { 2 } else { 1 }).unwrap();
        assert_eq!(old.get(), 1);
        if let Some(new) = new {
            assert_eq!((new.get(), machine.cpu().eip), (1, 0x200));
        } else {
            assert_eq!((machine.cpu().registers.eax, machine.cpu().eip), (1, 0x101));
        }
    }
}

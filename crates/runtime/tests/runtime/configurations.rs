use super::*;
use wasm86_machine::MemoryError;
use wasm86_x86::PagedMemoryMap;
use wasm86_x86::{
    Compiler, CpuFeature, CpuModel, CpuState, DescriptorTables, Exception, ExecutionExit,
    ExecutionTarget, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorInfo,
    SegmentDescriptorKind, SegmentLimit, SegmentProfile, StoredSegment,
};

#[derive(Default)]
struct Descriptors {
    tables: DescriptorTables,
    loads: u32,
    queries: u32,
}
impl Environment for Descriptors {
    fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: u16,
    ) -> Result<StoredSegment, Exception> {
        self.loads += 1;
        self.tables.resolve_user_segment(segment, selector)
    }
    fn query_segment_descriptor(&mut self, selector: u16) -> SegmentDescriptorInfo {
        self.queries += 1;
        self.tables.query_user_segment_descriptor(selector)
    }
}
fn descriptor(base: u32) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xfff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}
fn flat_machine<E: Environment>(environment: E) -> Machine<E> {
    flat_machine_for(environment, Compiler::new(SegmentProfile::Flat32).target())
}
fn flat_machine_for<E: Environment>(environment: E, target: ExecutionTarget) -> Machine<E> {
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    map.map(0x80001000..=0x80001fff, 0x2000, true).unwrap();
    let cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    Machine::new(target, cpu, map, environment).unwrap()
}

#[pollster::test]
async fn one_runtime_executes_real_and_flat_machines_with_private_backing() {
    let runtime = runtime().await;
    let mut real = runtime.attach(real_machine(ram(), ())).await.unwrap();
    real.write_memory(0, &[0xb8, 7, 0, 0xeb, 0]).unwrap();
    // MOV EAX, [80001000h] accesses high linear addresses through remapped backing.
    let mut flat = runtime.attach(flat_machine(())).await.unwrap();
    flat.write_memory(0x1000, &[0xa1, 0, 0x10, 0, 0x80, 0xeb, 0])
        .unwrap();
    flat.write_memory(0x80001000, &42u32.to_le_bytes()).unwrap();
    assert_eq!(flat.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(flat.cpu().registers.eax, 42);
    assert_eq!(real.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(real.cpu().registers.eax, 7);
    assert_eq!(flat.cpu().eip, 0x1007);
    assert_eq!(real.cpu().eip, 5);
    flat.write_memory(0x80001000, &[9]).unwrap();
    let mut byte = [0];
    real.read_memory(0, &mut byte).unwrap();
    assert_eq!(byte, [0xb8]);
}

#[pollster::test]
async fn protected_descriptor_callbacks_load_fs_and_query_access_rights() {
    let mut environment = Descriptors::default();
    environment.tables.insert(0x23, descriptor(0x80001000));
    // MOV AX,23h; MOV FS,AX; LAR EDX,AX; MOV EAX,FS:[0]; UD2
    let code = [
        0x66, 0xb8, 0x23, 0, 0x8e, 0xe0, 0x0f, 0x02, 0xd0, 0x64, 0xa1, 0, 0, 0, 0, 0x0f, 0x0b,
    ];
    let mut session = runtime()
        .await
        .attach(flat_machine(environment))
        .await
        .unwrap();
    session.write_memory(0x1000, &code).unwrap();
    session
        .write_memory(0x80001000, &42u32.to_le_bytes())
        .unwrap();
    let outcome = session.run(20).unwrap();
    assert_eq!(session.cpu().instruction_count, 4);
    assert_eq!(outcome, ExecutionExit::Exception(Exception::InvalidOpcode));
    assert_eq!(session.cpu().registers.eax, 42);
    assert_eq!(session.cpu().registers.edx & 0xff00, 0xf300);
    assert_eq!(session.cpu().segments.fs.base, 0x80001000);
    assert_eq!(
        session.with_environment(|env| (env.loads, env.queries)),
        (1, 1)
    );
}

#[pollster::test]
async fn paged_permissions_and_top_address_are_preserved_in_both_access_paths() {
    let mut session = runtime().await.attach(flat_machine(())).await.unwrap();
    session
        .write_memory(0x1000, &[0xa1, 0, 0x10, 0, 0x80, 0xeb, 0])
        .unwrap();
    session
        .write_memory(0x80001000, &42u32.to_le_bytes())
        .unwrap();
    let mut map = PagedMemoryMap::default();
    map.map(0x1000..=0x1fff, 0, true).unwrap();
    map.map(0x80001000..=0x80001fff, 0x2000, false).unwrap();
    map.map(0xfffff000..=u32::MAX, 0x3000, true).unwrap();
    session.with_memory(|memory| memory.set_map(map)).unwrap();
    assert_eq!(
        session.write_memory(0x80001000, &[1]),
        Err(MemoryError::ReadOnly {
            address: 0x80001000
        })
    );
    session.write_memory(u32::MAX, &[9]).unwrap();
    let mut byte = [0];
    session.read_memory(u32::MAX, &mut byte).unwrap();
    assert_eq!(byte, [9]);
    assert_eq!(session.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(session.cpu().registers.eax, 42);
    session
        .write_memory(0x1007, &[0xa3, 0, 0x10, 0, 0x80])
        .unwrap();
    assert_eq!(
        session.run(1).unwrap(),
        ExecutionExit::Exception(Exception::PageFault {
            linear_address: 0x80001000,
            error_code: 3
        })
    );
    assert_eq!(session.cpu().eip, 0x1007);
}

#[pollster::test]
async fn prepared_code_preserves_cpu_capabilities() {
    let runtime = runtime().await;
    let code = [0x0f, 0x44, 0xc0, 0xeb, 0]; // CMOVE EAX,EAX; JMP to the next block
    let mut enabled = runtime.attach(flat_machine(())).await.unwrap();
    enabled.write_memory(0x1000, &code).unwrap();
    let compiler = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false));
    let mut disabled = runtime
        .attach(flat_machine_for((), compiler.target()))
        .await
        .unwrap();
    disabled.write_memory(0x1000, &code).unwrap();
    assert_eq!(enabled.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        disabled.run(1).unwrap(),
        ExecutionExit::Exception(Exception::InvalidOpcode)
    );
    assert_eq!(disabled.cpu().eip, 0x1000);
}

#[pollster::test]
async fn rejected_descriptor_load_preserves_the_destination_segment() {
    // MOV AX,23h; MOV DS,AX with no matching descriptor.
    let mut session = runtime()
        .await
        .attach(flat_machine(Descriptors::default()))
        .await
        .unwrap();
    session
        .write_memory(0x1000, &[0x66, 0xb8, 0x23, 0, 0x8e, 0xd8])
        .unwrap();
    let ds = session.cpu().segments.ds;
    let outcome = session.run(10).unwrap();
    assert_eq!(session.cpu().instruction_count, 1);
    assert_eq!(
        outcome,
        ExecutionExit::Exception(Exception::GeneralProtection { error_code: 0x20 })
    );
    assert_eq!(session.cpu().segments.ds, ds);
    assert_eq!(session.cpu().eip, 0x1004);
}

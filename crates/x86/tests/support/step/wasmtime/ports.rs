use super::*;
use ::wasmtime::{Func, Ref, RefType, Table, TableType};

// Distinct entries make the existing scalar/string cases observe the table index,
// independently of the port argument delivered to the callback.
const BOUND_PORTS: [u32; 3] = [0x92, 0xff, 0xffff];

pub(super) fn register(linker: &mut Linker<ExecutionEvents>, store: &mut Store<ExecutionEvents>) {
    for write in [false, true] {
        let default = callback(store, write, None);
        let table = Table::new(
            &mut *store,
            TableType::new(RefType::FUNCREF, 65536, Some(65536)),
            Ref::Func(Some(default)),
        )
        .unwrap();
        for port in BOUND_PORTS {
            let function = callback(store, write, Some(port));
            table
                .set(&mut *store, u64::from(port), Ref::Func(Some(function)))
                .unwrap();
        }
        let name = if write { "portWrites" } else { "portReads" };
        linker.define(&*store, "relocade", name, table).unwrap();
    }
}

fn callback(store: &mut Store<ExecutionEvents>, write: bool, slot: Option<u32>) -> Func {
    let check = move |port| {
        assert_eq!(
            slot,
            BOUND_PORTS.contains(&port).then_some(port),
            "port table index"
        );
    };
    if write {
        Func::wrap(
            store,
            move |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32, value: u32| {
                check(port);
                caller
                    .data_mut()
                    .events
                    .push(Event::PortWrite { port, bytes, value });
            },
        )
    } else {
        Func::wrap(
            store,
            move |mut caller: Caller<'_, ExecutionEvents>, port: u32, bytes: u32| {
                check(port);
                let events = caller.data_mut();
                events.events.push(Event::PortRead { port, bytes });
                events.port_reads.next().expect("unexpected port read")
            },
        )
    }
}

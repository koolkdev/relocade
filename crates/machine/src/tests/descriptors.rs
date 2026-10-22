use super::support::{self, Configuration};

use super::support::Fixture;
use crate::{ConfigurationError, DescriptorError, DescriptorMemory, PagedMemoryMap};
use relocade_x86::{
    Compiler, CpuState, Descriptor, DescriptorTableRegister, DescriptorTables, Exception, Segment,
    SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit, SegmentProfile,
    SystemSegmentRegister,
};

fn data(base: u32) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xffff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}

fn config() -> Configuration {
    let mut tables = DescriptorTables::default();
    tables.insert(0x23, data(0x400000));
    tables.insert(0x0f, data(0x500000));
    support::with_descriptors(
        support::configuration(
            Compiler::new(SegmentProfile::Flat32).target(),
            CpuState::default(),
            0,
            PagedMemoryMap::default(),
        )
        .unwrap(),
        tables,
        Some(0x28),
    )
    .unwrap()
}

#[test]
fn dedicated_table_storage_needs_no_guest_backing_or_mapping() {
    let config = config();
    assert_eq!(config.machine.memory_size(), 0);
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    {
        let resolved = machine
            .descriptors()
            .resolve_user_segment(Segment::Fs, 0x23)
            .unwrap();
        assert_eq!(
            (resolved.cache().base, resolved.cache().selector),
            (0x400000, 0x23)
        );
        assert!(!resolved.needs_completion());
    }
    assert_eq!(machine.descriptors().read(0x0c).unwrap().base(), 0x500000);
    assert_eq!(
        machine.cpu().gdtr(),
        DescriptorTableRegister::new(0, 0xffff)
    );
    assert_eq!(machine.cpu().ldtr().selector, 0x28);
}

#[test]
fn lookup_uses_live_register_bases_limits_and_the_loaded_ldtr_cache() {
    let config = config();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    let original_ldtr = machine.cpu().ldtr();
    machine
        .descriptors()
        .write(0x28, Descriptor::ldt(0x18000, 7))
        .unwrap();
    assert_eq!(machine.cpu().ldtr(), original_ldtr);
    assert_eq!(machine.descriptors().read(0x0f).unwrap().base(), 0x500000);
    machine
        .descriptors()
        .write_at(0x1020, data(0x600000))
        .unwrap();
    machine
        .cpu()
        .set_gdtr(DescriptorTableRegister::new(0x1000, 0x27));
    assert_eq!(machine.descriptors().read(0x20).unwrap().base(), 0x600000);
    machine
        .cpu()
        .set_gdtr(DescriptorTableRegister::new(0x1000, 0x26));
    assert_eq!(machine.descriptors().read(0x23), None);
    assert_eq!(
        machine
            .descriptors()
            .resolve_user_segment(Segment::Fs, 0x23)
            .err(),
        Some(Exception::GeneralProtection { error_code: 0x20 })
    );
    for base in [DescriptorMemory::BYTE_LEN - 0x27, u32::MAX - 7] {
        machine
            .cpu()
            .set_gdtr(DescriptorTableRegister::new(base, 0xffff));
        assert_eq!(machine.descriptors().read(0x23), None);
    }
    machine
        .cpu()
        .set_ldtr(SystemSegmentRegister::default())
        .unwrap();
    assert!(
        !machine
            .descriptors()
            .query_user_segment_descriptor(0x0f)
            .lar_valid
    );
    assert_eq!(
        machine
            .descriptors()
            .resolve_user_segment(Segment::Ds, 0)
            .unwrap()
            .cache()
            .usable,
        0
    );
}

#[test]
fn edits_and_queries_preserve_loaded_caches_and_resolution_defers_completion() {
    let config = config();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    let cache = machine
        .descriptors()
        .resolve_user_segment(Segment::Fs, 0x23)
        .unwrap()
        .cache();
    machine.cpu().set_segment(Segment::Fs, cache);
    let mut bytes = Descriptor::from(data(0x700000)).to_bytes();
    bytes[5] &= !1;
    machine
        .descriptors()
        .write(0x23, Descriptor::from_bytes(bytes))
        .unwrap();
    assert_eq!(machine.cpu().segment(Segment::Fs), cache);
    assert_eq!(
        machine
            .descriptors()
            .query_user_segment_descriptor(0x23)
            .access_rights,
        0x0040_f200
    );
    {
        let resolved = machine
            .descriptors()
            .resolve_user_segment(Segment::Fs, 0x23)
            .unwrap();
        assert_eq!(resolved.cache().base, 0x700000);
        assert!(resolved.needs_completion());
    }
    assert_eq!(machine.descriptors().read(0x23).unwrap().to_bytes(), bytes);
    let completed = machine
        .descriptors()
        .resolve_user_segment(Segment::Fs, 0x23)
        .unwrap()
        .complete();
    assert_eq!(completed.base, 0x700000);
    assert_eq!(completed.selector, 0x23);
    bytes[5] |= 1;
    assert_eq!(machine.descriptors().read(0x23).unwrap().to_bytes(), bytes);
    assert_eq!(machine.cpu().segment(Segment::Fs), cache);
    let snapshot = machine.cpu().snapshot();
    machine.descriptors().write(0x23, data(0x800000)).unwrap();
    machine.cpu().restore(&snapshot).unwrap();
    assert_eq!(machine.cpu().segment(Segment::Fs), cache);
    assert_eq!(machine.descriptors().read(0x23).unwrap().base(), 0x800000);
}

#[test]
fn segment_load_completion_uses_the_resolved_selector_in_the_active_table() {
    let config = config();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    // Both tables can start at any byte in dedicated descriptor memory. TI selects the
    // table, and RPL does not change which descriptor receives the accessed bit.
    machine
        .cpu()
        .set_gdtr(DescriptorTableRegister::new(0x1003, 0x27));
    let ldtr = SystemSegmentRegister {
        base: 0x18005,
        limit: 0x0f,
        ..machine.cpu().ldtr()
    };
    machine.cpu().set_ldtr(ldtr).unwrap();
    for (selector, address) in [(0x20, 0x1023), (0x0d, 0x1800d)] {
        let mut bytes = Descriptor::from(data(0x700000)).to_bytes();
        bytes[5] &= !1;
        machine
            .descriptors()
            .write_at(address, Descriptor::from_bytes(bytes))
            .unwrap();
        let completion_selector = {
            let load = machine
                .descriptors()
                .resolve_user_segment(Segment::Fs, selector)
                .unwrap();
            assert!(load.needs_completion());
            assert_eq!(load.cache().selector, selector);
            load.cache().selector
        };
        assert_eq!(
            machine.descriptors().read_at(address).unwrap().to_bytes(),
            bytes
        );
        // The Wasm boundary transports the checked cache and a Boolean; the
        // completion callback receives that cache's selector.
        machine
            .descriptors()
            .mark_accessed(completion_selector)
            .unwrap();
        bytes[5] |= 1;
        assert_eq!(
            machine.descriptors().read_at(address).unwrap().to_bytes(),
            bytes
        );
        assert!(!machine
            .descriptors()
            .resolve_user_segment(Segment::Fs, selector)
            .unwrap()
            .needs_completion());
    }
    let null = machine
        .descriptors()
        .resolve_user_segment(Segment::Ds, 0)
        .unwrap();
    assert!(!null.needs_completion());
    assert_eq!(null.complete().usable, 0);
}

#[test]
fn loaded_system_cache_edits_are_validated_without_reloading_tables() {
    let config = config();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    let original = machine.cpu().snapshot();
    let stale = SystemSegmentRegister {
        base: 0xffff0000,
        limit: 0x1234567,
        ..original.ldtr
    };
    machine.cpu().set_ldtr(stale).unwrap();
    assert_eq!(machine.cpu().ldtr(), stale);
    let mut invalid = stale;
    invalid.selector |= 4;
    assert_eq!(
        machine.cpu().set_ldtr(invalid),
        Err(ConfigurationError::InvalidTableRegister)
    );
    assert_eq!(machine.cpu().ldtr(), stale);
    let tr = SystemSegmentRegister {
        selector: 0x30,
        access_rights: 0x8b00,
        ..stale
    };
    machine.cpu().set_tr(tr).unwrap();
    assert_eq!(machine.cpu().tr(), tr);
}

#[test]
fn restore_rejects_invalid_system_segments_without_writing() {
    let config = config();
    let mut fixture = Fixture::new(&config.cpu, &config.machine);
    let mut machine = fixture.view(&config);
    let before = machine.cpu().snapshot();
    let invalid = SystemSegmentRegister {
        selector: 0x30,
        usable: 1,
        access_rights: 0x9300, // A data descriptor is not an LDT or TSS.
        ..SystemSegmentRegister::default()
    };
    for snapshot in [
        CpuState {
            ldtr: invalid,
            ..before
        },
        CpuState {
            tr: invalid,
            ..before
        },
    ] {
        assert_eq!(
            machine.cpu().restore(&snapshot),
            Err(ConfigurationError::InvalidTableRegister)
        );
        assert_eq!(machine.cpu().snapshot(), before);
    }
}

#[test]
fn hle_initialization_rejects_ldt_collisions() {
    let mut tables = DescriptorTables::default();
    tables.insert(0x28, data(0));
    assert!(matches!(
        support::with_descriptors(config(), tables, Some(0x2b)),
        Err(DescriptorError::OccupiedLdtSlot)
    ));
    let mut tables = DescriptorTables::default();
    tables.insert(7, data(0));
    assert!(matches!(
        support::with_descriptors(config(), tables, None),
        Err(DescriptorError::MissingLdtSelector)
    ));
}

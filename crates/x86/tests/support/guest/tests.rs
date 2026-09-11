use super::{Machine, Permissions};

#[test]
#[should_panic(expected = "conflicting permissions for guest page 4")]
fn disjoint_regions_cannot_assign_conflicting_permissions_to_one_page() {
    let mut machine = Machine::new(&[0x90]);
    machine.memory(0x4000, &[1], Permissions::ReadOnly);
    machine.memory(0x4010, &[2], Permissions::ReadWrite);
}

#[test]
#[should_panic(expected = "conflicting permissions for guest page 1")]
fn data_setup_cannot_silently_make_the_code_page_writable() {
    let mut machine = Machine::new(&[0x90]);
    machine.memory(0x1010, &[1], Permissions::ReadWrite);
}

#[test]
#[should_panic(expected = "fixture setup changed the instruction bytes")]
fn physical_alias_setup_cannot_change_the_interpreted_encoding() {
    let mut machine = Machine::with_mappings(
        0x1000,
        &[0x90],
        &[
            super::Mapping {
                page: 1,
                frame: 0x3000,
                permissions: Permissions::ReadOnly,
            },
            super::Mapping {
                page: 4,
                frame: 0x3000,
                permissions: Permissions::ReadWrite,
            },
        ],
    );
    machine.memory(0x4000, &[0x91], Permissions::ReadWrite);
    machine.run(
        crate::support::step::TestModule::interpreter(),
        crate::support::step::Engine::Wasmtime,
    );
}

#[path = "support/parallel.rs"]
mod support;

#[test]
fn lpt_files_share_a_reader_with_bios_and_direct_port_output() {
    pollster::block_on(support::parallel_file_access()).unwrap();
}

use super::Exception;

#[test]
fn supported_faults_have_architectural_vector_numbers() {
    for (exception, vector) in [
        (Exception::DivideError, 0),
        (Exception::SegmentNotPresent { error_code: 0u32 }, 11),
        (Exception::StackFault { error_code: 0 }, 12),
        (Exception::GeneralProtection { error_code: 0 }, 13),
        (
            Exception::PageFault {
                linear_address: 0,
                error_code: 0,
            },
            14,
        ),
    ] {
        assert_eq!(exception.vector() as u8, vector);
    }
}

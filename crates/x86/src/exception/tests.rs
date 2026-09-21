use super::Exception;

#[test]
fn supported_faults_have_architectural_vector_numbers() {
    for (exception, vector) in [
        (Exception::DivideError, 0),
        (Exception::BoundRangeExceeded, 5),
        (Exception::InvalidOpcode, 6),
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

#[test]
fn fault_names_without_error_codes() {
    for (exception, name) in [
        (Exception::<u32>::DivideError, "#DE"),
        (Exception::BoundRangeExceeded, "#BR"),
        (Exception::InvalidOpcode, "#UD"),
    ] {
        assert_eq!(exception.to_string(), name);
    }
}

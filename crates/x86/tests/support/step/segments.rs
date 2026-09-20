//! Both engines consume replies from the same host descriptor view.

use serde::Serialize;
use wasm86_x86::{DescriptorTables, Exception, Segment};

#[derive(Clone, Serialize)]
pub(crate) struct SegmentQuery {
    pub(crate) selector: u16,
    pub(crate) values: [u32; 5],
}

impl SegmentQuery {
    pub(crate) fn new(tables: &DescriptorTables, selector: u16) -> Self {
        let descriptor_info = tables.query_user_segment_descriptor(selector);
        Self {
            selector,
            values: [
                u32::from(descriptor_info.visible),
                u32::from(descriptor_info.readable),
                u32::from(descriptor_info.writable),
                descriptor_info.access_rights,
                descriptor_info.limit,
            ],
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct SegmentResolution {
    pub(crate) segment: u32,
    pub(crate) selector: u16,
    pub(crate) values: [u32; 6],
}

impl SegmentResolution {
    pub(crate) fn new(tables: &DescriptorTables, segment: Segment, selector: u16) -> Self {
        let values = match tables.resolve_user_segment(segment, selector) {
            Ok(cache) => [
                0,
                0,
                cache.base,
                cache.limit,
                u32::from(cache.selector),
                u32::from(cache.attributes.bits()),
            ],
            Err(exception) => {
                let error_code = match exception {
                    Exception::GeneralProtection { error_code }
                    | Exception::SegmentNotPresent { error_code }
                    | Exception::StackFault { error_code } => error_code,
                    _ => unreachable!("descriptor resolution returns segment faults"),
                };
                [exception.vector() as u32, error_code, 0, 0, 0, 0]
            }
        };
        Self {
            segment: segment as u32,
            selector,
            values,
        }
    }
}

use super::{PhysicalMapping, PhysicalMemoryMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl Serialize for PhysicalMemoryMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Contents<'a> {
            pages: &'a [PhysicalMapping],
            a20_enabled: bool,
        }
        Contents {
            pages: &self.pages,
            a20_enabled: self.a20_enabled,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PhysicalMemoryMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Contents {
            pages: Vec<PhysicalMapping>,
            a20_enabled: bool,
        }
        let contents = Contents::deserialize(deserializer)?;
        if contents.pages.len() != Self::PAGE_COUNT {
            return Err(serde::de::Error::custom(
                "physical map must contain 272 pages",
            ));
        }
        // Validate every raw page, including those hidden by the A20 gate.
        let mut map = Self::new(
            contents
                .pages
                .into_iter()
                .enumerate()
                .map(|(index, mapping)| {
                    let start = index as u32 * Self::PAGE_BYTES;
                    (start..=start + Self::PAGE_BYTES - 1, mapping)
                }),
        )
        .map_err(serde::de::Error::custom)?;
        map.set_a20_enabled(contents.a20_enabled);
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> PhysicalMemoryMap {
        PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x100000..=0x100fff,
                PhysicalMapping::Rom {
                    backing_offset: 0x2000,
                },
            ),
        ])
        .unwrap()
    }

    #[test]
    fn serialization_preserves_pages_hidden_by_a20() {
        for enabled in [false, true] {
            let mut original = map();
            original.set_a20_enabled(enabled);
            let encoded = serde_json::to_value(&original).unwrap();
            let mut decoded: PhysicalMemoryMap = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded.to_bytes(), original.to_bytes());
            decoded.set_a20_enabled(true);
            assert_eq!(
                decoded.get(0x100123),
                PhysicalMapping::Rom {
                    backing_offset: 0x2123
                }
            );
        }
    }

    #[test]
    fn decoding_validates_hidden_pages_and_table_length() {
        let encoded = serde_json::to_value(map()).unwrap();
        let mut unaligned = encoded.clone();
        unaligned["pages"][256]["Rom"]["backing_offset"] = 1.into();
        assert!(serde_json::from_value::<PhysicalMemoryMap>(unaligned).is_err());
        for length in [
            PhysicalMemoryMap::PAGE_COUNT - 1,
            PhysicalMemoryMap::PAGE_COUNT + 1,
        ] {
            let mut wrong_length = encoded.clone();
            wrong_length["pages"]
                .as_array_mut()
                .unwrap()
                .resize(length, serde_json::Value::String("Unmapped".into()));
            assert!(serde_json::from_value::<PhysicalMemoryMap>(wrong_length).is_err());
        }
    }
}

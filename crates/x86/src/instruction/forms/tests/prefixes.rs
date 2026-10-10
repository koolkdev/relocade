//! Mandatory selectors are independent of the code segment's operand width.

use super::*;
use crate::SegmentDefaultSize;

#[test]
fn mandatory_prefixes_select_one_form_and_reject_mixed_selectors() {
    let requirements = [
        MandatoryPrefix::None,
        MandatoryPrefix::P66,
        MandatoryPrefix::F2,
        MandatoryPrefix::F3,
    ];
    for default in [SegmentDefaultSize::Bits16, SegmentDefaultSize::Bits32] {
        for (prefixes, expected) in [
            (vec![], Some(0)),
            (vec![Prefix::OperandSize], Some(1)),
            (vec![Prefix::Group1(Group1Prefix::F2)], Some(2)),
            (vec![Prefix::Group1(Group1Prefix::F3)], Some(3)),
            (vec![Prefix::Group1(Group1Prefix::F0)], None),
            (
                vec![Prefix::OperandSize, Prefix::Group1(Group1Prefix::F2)],
                None,
            ),
            (
                vec![Prefix::Group1(Group1Prefix::F3), Prefix::OperandSize],
                None,
            ),
        ] {
            let prefixes = prefixes
                .into_iter()
                .fold(PrefixState::new(default), PrefixState::with_prefix);
            let matches = requirements
                .iter()
                .enumerate()
                .filter(|(_, requirement)| requirement.matches(&prefixes))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(matches, expected.into_iter().collect::<Vec<_>>());
        }
    }
}

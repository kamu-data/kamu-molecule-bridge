use std::str::FromStr;

use alloy::primitives::{Address, address};
use molecule_ocl::entities::{
    OclId, OclOwnershipChange, OclOwnershipDiffMap, OclOwnershipProjection,
    OclOwnershipProjectionMap, OclTransferEvent,
};
use pretty_assertions::assert_eq;

const ADDR_A: Address = address!("0x1111111111111111111111111111111111111111");
const ADDR_B: Address = address!("0x2222222222222222222222222222222222222222");
const ADDR_C: Address = address!("0x3333333333333333333333333333333333333333");

#[rstest::rstest]
#[case::no_events(
    vec![],
    expected_projections([]),
    expected_diff([]),
)]
#[case::single_transfer(
    vec![OclTransferEvent { ocl_id: ocl_id_1(), from: Address::ZERO, to: ADDR_A }],
    expected_projections([(
        ocl_id_1(),
        OclOwnershipProjection { current: Some(ADDR_A), previous: vec![] },
    )]),
    expected_diff([(
        ocl_id_1(),
        OclOwnershipChange { former_owner: None, current_owner: ADDR_A },
    )]),
)]
#[case::transfer_chain(
    vec![
        OclTransferEvent { ocl_id: ocl_id_1(), from: Address::ZERO, to: ADDR_A },
        OclTransferEvent { ocl_id: ocl_id_1(), from: ADDR_A, to: ADDR_B },
        OclTransferEvent { ocl_id: ocl_id_1(), from: ADDR_B, to: ADDR_C },
    ],
    expected_projections([(
        ocl_id_1(),
        OclOwnershipProjection { current: Some(ADDR_C), previous: vec![] },
    )]),
    expected_diff([(
        ocl_id_1(),
        OclOwnershipChange { former_owner: None, current_owner: ADDR_C },
    )]),
)]
#[case::transfer_back(
    vec![
        OclTransferEvent { ocl_id: ocl_id_1(), from: Address::ZERO, to: ADDR_A },
        OclTransferEvent { ocl_id: ocl_id_1(), from: ADDR_A, to: ADDR_B },
        OclTransferEvent { ocl_id: ocl_id_1(), from: ADDR_B, to: ADDR_A },
    ],
    expected_projections([(
        ocl_id_1(),
        OclOwnershipProjection { current: Some(ADDR_A), previous: vec![] },
    )]),
    expected_diff([(
        ocl_id_1(),
        OclOwnershipChange { former_owner: None, current_owner: ADDR_A },
    )]),
)]
#[case::multiple_ocls(
    vec![
        OclTransferEvent { ocl_id: ocl_id_1(), from: Address::ZERO, to: ADDR_A },
        OclTransferEvent { ocl_id: ocl_id_2(), from: Address::ZERO, to: ADDR_B },
    ],
    expected_projections([
        (ocl_id_1(), OclOwnershipProjection { current: Some(ADDR_A), previous: vec![] }),
        (ocl_id_2(), OclOwnershipProjection { current: Some(ADDR_B), previous: vec![] }),
    ]),
    expected_diff([
        (ocl_id_1(), OclOwnershipChange { former_owner: None, current_owner: ADDR_A }),
        (ocl_id_2(), OclOwnershipChange { former_owner: None, current_owner: ADDR_B }),
    ]),
)]
fn test_process_batch_events(
    #[case] events: Vec<OclTransferEvent>,
    #[case] expected_proj: OclOwnershipProjectionMap,
    #[case] expected_diff: OclOwnershipDiffMap,
) {
    let (proj, diff) = apply_events_to_projections(events);
    assert_eq!(expected_proj, proj);
    assert_eq!(expected_diff, diff);
}

#[test]
fn test_process_events_one_by_one() {
    let mut projections = OclOwnershipProjectionMap::default();

    {
        let diff = projections.apply_events(vec![OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: Address::ZERO,
            to: ADDR_A,
        }]);

        assert_eq!(
            expected_projections([(
                ocl_id_1(),
                OclOwnershipProjection {
                    current: Some(ADDR_A),
                    previous: vec![],
                }
            )]),
            projections
        );
        assert_eq!(
            expected_diff([(
                ocl_id_1(),
                OclOwnershipChange {
                    former_owner: None,
                    current_owner: ADDR_A,
                }
            )]),
            diff
        );
    }
    {
        let diff = projections.apply_events(vec![OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_A,
            to: ADDR_B,
        }]);

        assert_eq!(
            expected_projections([(
                ocl_id_1(),
                OclOwnershipProjection {
                    current: Some(ADDR_B),
                    previous: vec![ADDR_A],
                }
            )]),
            projections
        );
        assert_eq!(
            expected_diff([(
                ocl_id_1(),
                OclOwnershipChange {
                    former_owner: Some(ADDR_A),
                    current_owner: ADDR_B,
                }
            )]),
            diff
        );
    }
    {
        let diff = projections.apply_events(vec![OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_B,
            to: ADDR_C,
        }]);

        assert_eq!(
            expected_projections([(
                ocl_id_1(),
                OclOwnershipProjection {
                    current: Some(ADDR_C),
                    previous: vec![ADDR_A, ADDR_B],
                }
            )]),
            projections
        );
        assert_eq!(
            expected_diff([(
                ocl_id_1(),
                OclOwnershipChange {
                    former_owner: Some(ADDR_B),
                    current_owner: ADDR_C,
                }
            )]),
            diff
        );
    }
    {
        let diff = projections.apply_events(vec![OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_C,
            to: ADDR_A,
        }]);

        assert_eq!(
            expected_projections([(
                ocl_id_1(),
                OclOwnershipProjection {
                    current: Some(ADDR_A),
                    previous: vec![ADDR_B, ADDR_C],
                }
            )]),
            projections
        );
        assert_eq!(
            expected_diff([(
                ocl_id_1(),
                OclOwnershipChange {
                    former_owner: Some(ADDR_C),
                    current_owner: ADDR_A,
                }
            )]),
            diff
        );
    }
}

// Helpers

fn ocl_id_1() -> OclId {
    const OCL_ID_1: &str = "0x0101000000000000000000a1117b215dcd666dd847cfa84721480d316440faa9";
    OclId::from_str(OCL_ID_1).unwrap()
}

fn ocl_id_2() -> OclId {
    const OCL_ID_2: &str = "0x0101000000000000000000992399d367a2fa6f971dbc1647f81f999c19a70d67";
    OclId::from_str(OCL_ID_2).unwrap()
}

fn apply_events_to_projections(
    events: Vec<OclTransferEvent>,
) -> (OclOwnershipProjectionMap, OclOwnershipDiffMap) {
    let mut projections = OclOwnershipProjectionMap::default();
    let diff = projections.apply_events(events);
    (projections, diff)
}

fn expected_projections(
    entries: impl IntoIterator<Item = (OclId, OclOwnershipProjection)>,
) -> OclOwnershipProjectionMap {
    OclOwnershipProjectionMap::from_entries(entries)
}

fn expected_diff(
    entries: impl IntoIterator<Item = (OclId, OclOwnershipChange)>,
) -> OclOwnershipDiffMap {
    entries.into_iter().collect()
}

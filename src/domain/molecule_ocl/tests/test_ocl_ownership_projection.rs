use alloy::primitives::{Address, address};
use molecule_ocl::entities::{OclId, OclOwnershipProjection, OclTransferEvent, compress_events};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::str::FromStr;

const ADDR_A: Address = address!("0x1111111111111111111111111111111111111111");
const ADDR_B: Address = address!("0x2222222222222222222222222222222222222222");
const ADDR_C: Address = address!("0x3333333333333333333333333333333333333333");

struct ApplyTransferStep {
    new_owner: Address,
    expected_projection: OclOwnershipProjection,
    expected_previous_owner: Option<Address>,
}

#[rstest::rstest]
#[case::transfer_to_same_owner(vec![
    ApplyTransferStep {
        new_owner: ADDR_A,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_A),
            previous: vec![],
        },
        expected_previous_owner: None,
    },
    ApplyTransferStep {
        new_owner: ADDR_A,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_A),
            previous: vec![],
        },
        expected_previous_owner: None,
    },
])]
#[case::transfer_chain(vec![
    ApplyTransferStep {
        new_owner: ADDR_A,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_A),
            previous: vec![],
        },
        expected_previous_owner: None,
    },
    ApplyTransferStep {
        new_owner: ADDR_B,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_B),
            previous: vec![ADDR_A],
        },
        expected_previous_owner: Some(ADDR_A),
    },
    ApplyTransferStep {
        new_owner: ADDR_C,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_C),
            previous: vec![ADDR_A, ADDR_B],
        },
        expected_previous_owner: Some(ADDR_B),
    },
    ApplyTransferStep {
        new_owner: ADDR_B,
        expected_projection: OclOwnershipProjection {
            current: Some(ADDR_B),
            previous: vec![ADDR_A, ADDR_C],
        },
        expected_previous_owner: Some(ADDR_C),
    },
])]
fn test_apply_transfer(#[case] steps: Vec<ApplyTransferStep>) {
    let mut projection = OclOwnershipProjection::default();

    for step in steps {
        let previous_owner = projection.apply_transfer(step.new_owner);

        assert_eq!(step.expected_projection, projection);
        assert_eq!(step.expected_previous_owner, previous_owner);
    }
}

struct CompressEventsCase {
    events: Vec<OclTransferEvent>,
    expected: HashMap<OclId, Address>,
}

#[rstest::rstest]
#[case::empty_events(CompressEventsCase {
    events: vec![],
    expected: HashMap::new(),
})]
#[case::single_event(CompressEventsCase {
    events: vec![OclTransferEvent {
        ocl_id: ocl_id_1(),
        from: Address::ZERO,
        to: ADDR_A,
    }],
    expected: HashMap::from([(ocl_id_1(), ADDR_A)]),
})]
#[case::last_wins_same_ocl_id(CompressEventsCase {
    // A -> B -> C -> A -> B -> A -> B
    events: vec![
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_A,
            to: ADDR_B,
        },
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_B,
            to: ADDR_C,
        },
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_C,
            to: ADDR_A,
        },
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_A,
            to: ADDR_B,
        },
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_B,
            to: ADDR_A,
        },
    ],
    expected: HashMap::from([(ocl_id_1(), ADDR_A)]),
})]
#[case::interleaved_multiple_ocl_ids(CompressEventsCase {
    events: vec![
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: Address::ZERO,
            to: ADDR_A,
        },
        OclTransferEvent {
            ocl_id: ocl_id_2(),
            from: Address::ZERO,
            to: ADDR_B,
        },
        OclTransferEvent {
            ocl_id: ocl_id_1(),
            from: ADDR_A,
            to: ADDR_C,
        },
    ],
    expected: HashMap::from([(ocl_id_1(), ADDR_C), (ocl_id_2(), ADDR_B)]),
})]
fn test_compress_events(#[case] case: CompressEventsCase) {
    let actual = compress_events(case.events);

    assert_eq!(case.expected, actual);
}

// Helpers

fn ocl_id_1() -> OclId {
    const RAW: &str = "0x0101000000000000000000a1117b215dcd666dd847cfa84721480d316440faa9";
    OclId::from_str(RAW).unwrap()
}

fn ocl_id_2() -> OclId {
    const RAW: &str = "0x0101000000000000000000992399d367a2fa6f971dbc1647f81f999c19a70d67";
    OclId::from_str(RAW).unwrap()
}

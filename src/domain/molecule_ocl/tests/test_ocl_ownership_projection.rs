use alloy::primitives::{Address, address};
use molecule_ocl::entities::OclOwnershipProjection;
use pretty_assertions::assert_eq;

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

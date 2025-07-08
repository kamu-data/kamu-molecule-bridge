use std::collections::{HashMap, HashSet};

use alloy::primitives::{Address, U256, address};
use molecule_ipnft::entities::{IpnftEvent, IpnftEventMinted, IpnftEventTransfer, IpnftUid};
use molecule_ipnft::strategies::{IpnftEventProcessingDecision, IpnftEventProcessingStrategy};
use multisig::services::MockMultisigResolver;
use pretty_assertions::assert_eq;

type Harness = IpnftEventProcessingStrategyHarness;

#[tokio::test]
async fn test_initial_owner_transfers_to_multisig_they_participate_in() {
    let ipnft_uid = IpnftUid {
        ipnft_address: address!("0x1010101010101010101010101010101010101010"),
        token_id: U256::from(1),
    };

    let initial_owner = address!("0x2020202020202020202020202020202020202020");
    let multisig = address!("0x3030303030303030303030303030303030303030");
    let another_multisig_owner = address!("0x4040404040404040404040404040404040404040");

    let mut mock_multisig_resolver = MockMultisigResolver::new();
    Harness::expect_get_multisig_owners(
        &mut mock_multisig_resolver,
        [(
            multisig,
            HashSet::from([initial_owner, another_multisig_owner]),
        )]
        .into(),
    );

    let events = [
        IpnftEvent::Minted(IpnftEventMinted {
            ipnft_uid,
            initial_owner,
            symbol: "FOOBAR".to_string(),
        }),
        IpnftEvent::Transfer(IpnftEventTransfer {
            ipnft_uid,
            from: initial_owner,
            to: multisig,
        }),
    ];

    let mut decisions = IpnftEventProcessingStrategy::new(&mock_multisig_resolver)
        .process(&events)
        .await
        .unwrap();
    decisions.sort_unstable();

    assert_eq!(
        [
            IpnftEventProcessingDecision::GrantMaintainerAccess {
                ipnft_uid,
                address: initial_owner,
            },
            IpnftEventProcessingDecision::GrantMaintainerAccess {
                ipnft_uid,
                address: another_multisig_owner,
            }
        ],
        *decisions
    );
}

struct IpnftEventProcessingStrategyHarness;

impl IpnftEventProcessingStrategyHarness {
    fn expect_get_multisig_owners(
        mock: &mut MockMultisigResolver,
        mapping: HashMap<Address, HashSet<Address>>,
    ) {
        mock.expect_get_multisig_owners()
            .returning(move |address| Ok(mapping.get(&address).cloned()));
    }
}

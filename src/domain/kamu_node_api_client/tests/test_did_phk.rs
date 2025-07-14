use alloy::primitives::address;
use kamu_node_api_client::DidPhk;
use pretty_assertions::assert_eq;

#[test]
fn test_new_from_chain_id() {
    {
        let address = address!("0xabcdef1010101010101010101010101010101010");
        let did_phk = DidPhk::new_from_chain_id(1, address).unwrap();

        assert_eq!(
            "did:pkh:eip155:1:0xabCdeF1010101010101010101010101010101010",
            did_phk.to_string()
        );
    }
    {
        let address = address!("0xabcdef1010101010101010101010101010101010");
        let did_phk = DidPhk::new_from_chain_id(11155111, address).unwrap();

        assert_eq!(
            "did:pkh:eip155:11155111:0xabCdeF1010101010101010101010101010101010",
            did_phk.to_string()
        );
    }
}

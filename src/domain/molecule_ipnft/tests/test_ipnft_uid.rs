use std::str::FromStr;

use alloy::primitives::{U512, address};
use molecule_ipnft::entities::IpnftUid;
use pretty_assertions::assert_eq;

#[test]
fn test_display_and_parse_round_trip() {
    let original = IpnftUid {
        ipnft_address: address!("0x1010101010101010101010101010101010101010"),
        token_id: U512::from(123),
    };

    let displayed = original.to_string();
    let parsed = IpnftUid::from_str(&displayed).unwrap();

    assert_eq!(original, parsed);
}

#[test]
fn test_parse_invalid_format() {
    let e = IpnftUid::from_str("0x1010101010101010101010101010101010101010_123_").unwrap_err();

    assert_eq!(
        "Invalid format: '0x1010101010101010101010101010101010101010_123_'",
        e.to_string()
    );
}

#[test]
fn test_parse_invalid_address() {
    let e = IpnftUid::from_str("0xFOOBAR1010101010101010101010101010101010_123").unwrap_err();

    assert_eq!(
        "Address parse error: '0xFOOBAR1010101010101010101010101010101010'",
        e.to_string()
    );
}

#[test]
fn test_parse_invalid_token_id() {
    let e = IpnftUid::from_str("0x1010101010101010101010101010101010101010_123FOOBAR").unwrap_err();

    assert_eq!("Token ID parse error: '123FOOBAR'", e.to_string());
}

#[test]
fn test_large_token_id_parse() {
    let original = IpnftUid {
        ipnft_address: address!("0x1010101010101010101010101010101010101010"),
        token_id: U512::from_str(
            "187967774338575485875718808022503997019662064915219653732842355510261590999863",
        )
        .unwrap(),
    };

    let displayed = original.to_string();
    let parsed = IpnftUid::from_str(&displayed).unwrap();

    assert_eq!(original, parsed);
}

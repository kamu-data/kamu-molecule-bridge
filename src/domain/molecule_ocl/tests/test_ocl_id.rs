use std::str::FromStr;

use alloy::primitives::B256;
use molecule_ocl::entities::OclId;
use pretty_assertions::assert_eq;

#[test]
fn test_display_round_trip() {
    const OCL_ID_HEX_BYTES: &str =
        "0x0101000000000000000000a1117b215dcd666dd847cfa84721480d316440faa9";

    let ocl_id = OclId::new(B256::from_str(OCL_ID_HEX_BYTES).unwrap());

    assert_eq!(OCL_ID_HEX_BYTES, ocl_id.to_string());
}

#[test]
fn test_parse_round_trip() {
    const OCL_ID_HEX_BYTES: &str =
        "0x0101000000000000000000a1117b215dcd666dd847cfa84721480d316440faa9";

    let ocl_id: OclId = OCL_ID_HEX_BYTES.parse().unwrap();

    assert_eq!(OCL_ID_HEX_BYTES, ocl_id.to_string());
}

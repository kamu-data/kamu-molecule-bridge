use std::str::FromStr;

use molecule_ocl::entities::OclId;
use pretty_assertions::assert_eq;

#[test]
fn test_parse_round_trip() {
    let input = "0x0101000000000000000000a1117b215dcd666dd847cfa84721480d316440faa9";
    let parsed = OclId::from_str(input).unwrap();

    assert_eq!(input, parsed.to_string());
}

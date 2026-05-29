use std::str::FromStr;
use std::sync::LazyLock;

use alloy::primitives::B256;
use regex::Regex;

static OCL_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0x[0-9a-f]{64}$").unwrap());

#[nutype::nutype(
    validate(regex = OCL_ID_REGEX),
    derive(Debug, Display, AsRef, Clone, Eq, PartialEq, Hash, FromStr),
    cfg_attr(feature = "serde", derive(Serialize, Deserialize)),
)]
pub struct OclId(String);

impl OclId {
    pub fn try_from_b256(b256: B256) -> Result<Self, <Self as FromStr>::Err> {
        Self::from_str(&format!("{b256:#x}"))
    }
}

use alloy::primitives::B256;

#[nutype::nutype(
    derive(Debug, Display, AsRef, Copy, Clone, Eq, PartialEq, Hash, FromStr, From),
    cfg_attr(feature = "serde", derive(Serialize, Deserialize))
)]
pub struct OclId(B256);

pub mod prelude;

use alloy::sol;

sol!(LabNFT, "abis/LabNFT.json");

pub mod safe {
    use super::*;
    pub mod v1_3_0 {
        use super::*;
        sol!(Safe, "abis/Safe_1.3.0.json");
    }
    pub mod v1_5_0 {
        use super::*;
        sol!(Safe, "abis/Safe_1.5.0.json");
    }
}

// Actual version
pub use safe::v1_5_0::Safe;

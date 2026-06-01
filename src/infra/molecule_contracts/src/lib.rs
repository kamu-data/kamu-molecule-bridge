pub mod prelude;
mod safe_utils;

use alloy::sol;

sol!(
    // Generate Debug impls
    #[sol(all_derives = true)]
    LabNFT,
    "abis/LabNFT.json"
);

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

    pub use safe_utils::parse_safe_removed_owner_event;
}

// Actual version
pub use safe::v1_5_0::Safe;

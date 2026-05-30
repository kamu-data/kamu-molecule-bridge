use alloy::primitives::{Address, Log};

use crate::{Safe, safe};

pub fn parse_safe_removed_owner_event(log: &Log) -> eyre::Result<Address> {
    use alloy::sol_types::SolEvent;

    // First, try to parse the actual event signature (indexed "owner" field), ...
    let removed_owner = if let Ok(event) = Safe::RemovedOwner::decode_log(log) {
        event.owner
    } else {
        // Try to parse an old version event (w/o indexed mark) -- may be relevant for older Safe Wallet versions
        let event = safe::v1_3_0::Safe::RemovedOwner::decode_log(log)?;
        event.owner
    };

    Ok(removed_owner)
}

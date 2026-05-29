use std::collections::HashMap;

use alloy::primitives::Address;

use crate::entities::{
    OclId, OclOwnershipChange, OclOwnershipDiffMap, OclOwnershipProjection, OclTransferEvent,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct OclOwnershipProjectionMap {
    entries: HashMap<OclId, OclOwnershipProjection>,
}

impl OclOwnershipProjectionMap {
    pub fn from_entries<I: IntoIterator<Item = (OclId, OclOwnershipProjection)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }

    pub fn apply_events(&mut self, events: Vec<OclTransferEvent>) -> OclOwnershipDiffMap {
        let mut diff = OclOwnershipDiffMap::new();

        // NOTE: For a transfer chain A -> B -> C -> A -> B -> A -> B,
        //       we want to get only one change A -> B to eliminate redundant diff operations.
        let compressed_events = compress_events(events);

        for (ocl_id, last_owner) in compressed_events {
            use std::collections::hash_map::Entry;

            match self.entries.entry(ocl_id.clone()) {
                Entry::Occupied(mut e) => {
                    let projection = e.get_mut();
                    let Some(former_owner) = projection.apply_transfer(last_owner) else {
                        // No changes
                        continue;
                    };

                    diff.insert(
                        ocl_id,
                        OclOwnershipChange {
                            former_owner: Some(former_owner),
                            current_owner: last_owner,
                        },
                    );
                }
                Entry::Vacant(e) => {
                    let new_projection = OclOwnershipProjection::new(last_owner);
                    e.insert(new_projection);

                    diff.insert(
                        ocl_id,
                        OclOwnershipChange {
                            former_owner: None,
                            current_owner: last_owner,
                        },
                    );
                }
            }
        }

        diff
    }
}

// Helpers

type CompressedEvents = HashMap<OclId, Address>;

// TODO: verify from/previous owner?
// Public only for tests
pub fn compress_events(events: Vec<OclTransferEvent>) -> CompressedEvents {
    let mut res = HashMap::new();
    for OclTransferEvent { ocl_id, to, .. } in events {
        res.insert(ocl_id, to);
    }
    res
}

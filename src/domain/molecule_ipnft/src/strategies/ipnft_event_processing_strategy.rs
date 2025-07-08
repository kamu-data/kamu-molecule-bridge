use std::collections::{HashMap, HashSet};

use alloy::primitives::Address;
use color_eyre::eyre;
use multisig::services::MultisigResolver;

use crate::entities::{IpnftEvent, IpnftUid};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IpnftEventProcessingDecision {
    GrantMaintainerAccess {
        ipnft_uid: IpnftUid,
        address: Address,
    },
    RevokeMaintainerAccess {
        ipnft_uid: IpnftUid,
        address: Address,
    },
}

pub struct IpnftEventProcessingStrategy<'a> {
    multisig_resolver: &'a dyn MultisigResolver,
}

impl<'a> IpnftEventProcessingStrategy<'a> {
    pub fn new(multisig_resolver: &'a dyn MultisigResolver) -> Self {
        Self { multisig_resolver }
    }

    pub async fn process(
        &self,
        events: &[IpnftEvent],
    ) -> eyre::Result<Vec<IpnftEventProcessingDecision>> {
        #[derive(Debug, Default)]
        struct IpnftProjection {
            current_owners: Option<HashSet<Address>>,
            former_owners: HashSet<Address>,
        }

        let mut ipnft_projections_map = HashMap::<IpnftUid, IpnftProjection>::new();

        for event in events {
            let ipnft_projection: &mut IpnftProjection =
                ipnft_projections_map.entry(event.ipnft_uid()).or_default();

            match event {
                IpnftEvent::Minted(minted) => {
                    let initial_owners = self.get_owners(minted.initial_owner).await?;

                    ipnft_projection.current_owners = Some(initial_owners);
                }
                IpnftEvent::Transfer(transfer) => {
                    let previous_owners = self.get_owners(transfer.from).await?;
                    let new_owners = self.get_owners(transfer.to).await?;

                    // Add previous owners to former owners
                    ipnft_projection.former_owners.extend(previous_owners);

                    // Remove new owners from former owners
                    ipnft_projection
                        .former_owners
                        .retain(|former_owner| !new_owners.contains(former_owner));

                    ipnft_projection.current_owners = Some(new_owners);
                }
                IpnftEvent::Burnt(burnt) => {
                    let previous_owners = self.get_owners(burnt.former_owner).await?;

                    ipnft_projection.current_owners = None;
                    ipnft_projection.former_owners.extend(previous_owners);
                }
            }
        }

        let mut result_decisions = Vec::with_capacity(events.len());

        for (ipnft_uid, projection) in ipnft_projections_map {
            if let Some(current_owners) = projection.current_owners {
                result_decisions.extend(current_owners.into_iter().map(|current_owner| {
                    IpnftEventProcessingDecision::GrantMaintainerAccess {
                        ipnft_uid,
                        address: current_owner,
                    }
                }));
            }

            result_decisions.extend(projection.former_owners.into_iter().map(|former_owner| {
                IpnftEventProcessingDecision::RevokeMaintainerAccess {
                    ipnft_uid,
                    address: former_owner,
                }
            }));
        }

        Ok(result_decisions)
    }

    async fn get_owners(&self, address: Address) -> eyre::Result<HashSet<Address>> {
        let maybe_multisig_owners = self.multisig_resolver.get_multisig_owners(address).await?;
        let multisig_owners = maybe_multisig_owners.unwrap_or_else(|| HashSet::from([address]));

        Ok(multisig_owners)
    }
}

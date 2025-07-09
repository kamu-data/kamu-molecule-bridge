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
    AddMultisigToIndexing {
        ipnft_uid: IpnftUid,
        address: Address,
    },
    RemoveMultisigFromIndexing {
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
            symbol: Option<String>,
            current_owner: Option<Address>,
            former_owners: HashSet<Address>,
            minted: bool,
            burnt: bool,
        }

        let mut ipnft_projections_map = HashMap::<IpnftUid, IpnftProjection>::new();

        for event in events {
            let ipnft_projection: &mut IpnftProjection =
                ipnft_projections_map.entry(event.ipnft_uid()).or_default();

            match event {
                IpnftEvent::Minted(event) => {
                    ipnft_projection.symbol = Some(event.symbol.clone());
                    ipnft_projection.current_owner = Some(event.initial_owner);
                    ipnft_projection.minted = true;
                }
                IpnftEvent::Transfer(event) => {
                    // NOTE: If IPNFT was just minted, we haven't granted access yet,
                    //       so no need to revoke them.
                    if !ipnft_projection.minted {
                        ipnft_projection.former_owners.insert(event.from);
                    }

                    ipnft_projection.current_owner = Some(event.to);
                }
                IpnftEvent::Burnt(event) => {
                    ipnft_projection.current_owner = None;
                    ipnft_projection.former_owners.insert(event.former_owner);
                    ipnft_projection.burnt = true;
                }
            }
        }

        let mut decisions = Vec::with_capacity(events.len());

        for (ipnft_uid, projection) in ipnft_projections_map {
            if projection.minted && projection.burnt {
                // NOTE: IPNFT was burned before we could give access to anyone.
                //       So there's no need to revoke access from anyone as well.
                // TODO: Add debug log
                continue;
            }

            if let Some(current_owner) = projection.current_owner {
                let GetOwnersResponse { owners, multisig } = self.get_owners(current_owner).await?;

                decisions.extend(owners.into_iter().map(|current_owner| {
                    IpnftEventProcessingDecision::GrantMaintainerAccess {
                        ipnft_uid,
                        address: current_owner,
                    }
                }));

                if multisig {
                    decisions.push(IpnftEventProcessingDecision::AddMultisigToIndexing {
                        ipnft_uid,
                        address: current_owner,
                    });
                }
            }

            for former_owner in projection.former_owners {
                let GetOwnersResponse { owners, multisig } = self.get_owners(former_owner).await?;

                decisions.extend(owners.into_iter().map(|former_owner| {
                    IpnftEventProcessingDecision::RevokeMaintainerAccess {
                        ipnft_uid,
                        address: former_owner,
                    }
                }));

                if multisig {
                    decisions.push(IpnftEventProcessingDecision::RemoveMultisigFromIndexing {
                        ipnft_uid,
                        address: former_owner,
                    });
                }
            }
        }

        Ok(decisions)
    }

    async fn get_owners(&self, address: Address) -> eyre::Result<GetOwnersResponse> {
        let maybe_owners = self.multisig_resolver.get_multisig_owners(address).await?;
        let multisig = maybe_owners.is_some();
        let owners = maybe_owners.unwrap_or_else(|| HashSet::from([address]));

        Ok(GetOwnersResponse { owners, multisig })
    }
}

struct GetOwnersResponse {
    owners: HashSet<Address>,
    multisig: bool,
}

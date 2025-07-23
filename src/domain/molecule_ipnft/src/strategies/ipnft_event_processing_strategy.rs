use std::collections::HashMap;

use crate::entities::{IpnftEvent, IpnftEventProjection, IpnftUid};

pub type IpnftEventProjectionMap = HashMap<IpnftUid, IpnftEventProjection>;

pub struct IpnftEventProcessingStrategy;

impl IpnftEventProcessingStrategy {
    pub fn process(&self, events: Vec<IpnftEvent>) -> IpnftEventProjectionMap {
        // NOTE: Event projections on the current `events` group
        let mut iteration_projections_map = IpnftEventProjectionMap::new();

        for event in events {
            let projection = iteration_projections_map
                .entry(event.ipnft_uid())
                .or_default();

            match event {
                IpnftEvent::Minted(event) => {
                    projection.symbol = Some(event.symbol);
                    projection.current_owner = Some(event.initial_owner);
                    projection.minted = true;
                }
                IpnftEvent::Transfer(event) => {
                    projection.current_owner = Some(event.to);
                    projection.former_owner = Some(event.from);
                }
                IpnftEvent::Burnt(event) => {
                    projection.current_owner = None;
                    projection.former_owner = Some(event.former_owner);
                    projection.burnt = true;
                }
            }
        }

        iteration_projections_map
    }

    pub fn synchronize_ipnft_event_projections(
        &self,
        global_projection: &mut IpnftEventProjection,
        iteration_projection: IpnftEventProjection,
    ) {
        if iteration_projection.minted {
            global_projection.minted = true;
        }

        if let Some(symbol) = iteration_projection.symbol {
            debug_assert!(global_projection.symbol.is_none());
            global_projection.symbol = Some(symbol);
        }

        if let Some(new_current_owner) = iteration_projection.current_owner {
            global_projection.current_owner = Some(new_current_owner);
        }

        if let Some(new_former_owner) = iteration_projection.former_owner {
            global_projection.former_owner = Some(new_former_owner);
        }

        global_projection.burnt = iteration_projection.burnt;
    }

    pub fn synchronize_ipnft_event_projections_maps(
        &self,
        global_projections_map: &mut HashMap<IpnftUid, IpnftEventProjection>,
        iteration_projections_map: HashMap<IpnftUid, IpnftEventProjection>,
    ) {
        for (ipnft_uid, iteration_projection) in iteration_projections_map {
            let global_projection = global_projections_map.entry(ipnft_uid).or_default();

            self.synchronize_ipnft_event_projections(global_projection, iteration_projection);
        }
    }
}

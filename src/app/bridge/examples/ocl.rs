// ```shell
// cargo run -p kamu-molecule-bridge --example ocl
// ```

use std::collections::{HashMap, HashSet};

use alloy::primitives::{Address, B256};
use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::Log;
use alloy::sol_types::SolEvent;
use alloy_ext::prelude::*;
use color_eyre::eyre;
use molecule_contracts::LabNFT;

const PRINT_EVENTS: bool = false;

const INDEX_EVENTS: [B256; 1] = [LabNFT::OclTransfer::SIGNATURE_HASH];

#[derive(confique::Config, Debug)]
struct Config {
    #[config(env = "OCL_RPC_URL")]
    rpc_url: String,

    #[config(env = "OCL_LABNFT_ADDRESS")]
    labnft_address: Address,

    #[config(env = "OCL_LABNFT_FROM_BLOCK")]
    #[config(default = 0)]
    from_block: u64,
}

impl Config {
    fn builder() -> confique::Builder<Config> {
        confique::Config::builder()
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    for path in ["src/app/bridge/examples/ocl.env", "ocl.env"] {
        if dotenv::from_filename(path).is_ok() {
            break;
        }
    }

    let config = Config::builder().env().load()?;

    let provider = ProviderBuilder::new()
        .disable_recommended_fillers()
        .filler(ChainIdFiller::default())
        .connect_http(config.rpc_url.parse()?)
        .erased();

    let latest = provider.get_block_number().await?;

    let mut logs = Vec::new();
    provider
        .get_logs_ext(
            vec![config.labnft_address],
            HashSet::from(INDEX_EVENTS),
            config.from_block,
            latest,
            &mut |chunk| {
                logs.extend(chunk.logs);
                Ok(())
            },
        )
        .await?;

    println!(
        "Retrieved {} logs in blocks [{}..{latest}]\n",
        logs.len(),
        config.from_block
    );

    // Note: no burn function
    // https://github.com/moleculeprotocol/onchainlabs/blob/c69b3774a887906a3a05983d4a410847a189a779/docs/nft/labnft-solady-migration-plan.md?plain=1#L704

    let mut registry = OclRegistry::default();

    for log in &logs {
        use LabNFT::OclTransfer;

        match log.event_signature_hash() {
            OclTransfer::SIGNATURE_HASH => {
                let transfer = decode_event::<OclTransfer>(log)?;
                registry.apply_transfer(&transfer);
            }
            _ => unreachable!(),
        }
    }

    println!("OCL ownership map ({} entries):\n", registry.order.len());
    for ocl_id in &registry.order {
        let ownership = &registry.entries[ocl_id];
        println!(
            "- ocl_id = {ocl_id:#x}\n  current = {}\n  previous = {:?}\n",
            ownership
                .current
                .map(|address| format!("{address:#x}"))
                .unwrap_or_else(|| "<none>".into()),
            ownership
                .previous
                .iter()
                .map(|address| format!("{address:#x}"))
                .collect::<Vec<_>>(),
        );
    }

    Ok(())
}

#[derive(Debug, Default)]
struct OclOwnership {
    current: Option<Address>,
    previous: Vec<Address>,
}

#[derive(Debug, Default)]
struct OclRegistry {
    // Use indexmap in app
    order: Vec<B256>,
    entries: HashMap<B256, OclOwnership>,
}

impl OclRegistry {
    fn apply_transfer(&mut self, transfer: &LabNFT::OclTransfer) {
        use std::collections::hash_map::Entry;

        match self.entries.entry(transfer.oclId) {
            Entry::Vacant(entry) => {
                self.order.push(transfer.oclId);

                entry.insert(OclOwnership {
                    current: Some(transfer.to),
                    previous: Vec::new(),
                });
            }
            Entry::Occupied(mut entry) => {
                let ownership = entry.get_mut();

                let new_current = transfer.to;

                ownership
                    .previous
                    .retain(|previous| *previous != new_current);

                if let Some(old_current) = ownership.current {
                    ownership.previous.push(old_current);
                }

                ownership.current = Some(new_current);
            }
        }
    }
}

fn decode_event<E>(log: &Log) -> eyre::Result<E>
where
    E: SolEvent + std::fmt::Debug,
{
    let event = E::decode_log(&log.inner)?;

    if PRINT_EVENTS {
        let block = log.block_number.unwrap();
        let index = log.log_index.unwrap();
        println!(
            "- {}\n(block: {block}, index: {index}): {:#?}\n",
            E::SIGNATURE,
            event.data
        );
    }

    Ok(event.data)
}

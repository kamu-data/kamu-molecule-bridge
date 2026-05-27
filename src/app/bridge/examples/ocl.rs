// ```shell
// cargo run -p kamu-molecule-bridge --example ocl
// ```

use std::collections::{HashMap, HashSet};

use alloy::primitives::{Address, B256};
use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use alloy_ext::prelude::*;
use color_eyre::eyre;

const PRINT_EVENTS: bool = true;

sol!(
    // Generate Debug impls
    #[sol(all_derives = true)]
    LabNFT,
    "./examples/abi/LabNFT.json"
);

const INDEX_EVENTS: [B256; 2] = [
    LabNFT::Transfer::SIGNATURE_HASH,
    LabNFT::OclTransfer::SIGNATURE_HASH,
];

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

    use LabNFT::{OclTransfer, Transfer};

    type OclBalances = HashMap<Address, B256>;
    type OclState = HashMap<String, OclBalances>;

    let ocl_state: OclState = HashMap::new();

    for log in &logs {
        match log.event_signature_hash() {
            Transfer::SIGNATURE_HASH => {
                decode_event::<Transfer>(log)?;
            }
            OclTransfer::SIGNATURE_HASH => {
                decode_event::<OclTransfer>(log)?;
            }
            _ => unreachable!(),
        }
    }

    Ok(())
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

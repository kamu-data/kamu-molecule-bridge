// ```shell
// cargo run -p kamu-molecule-bridge --example ocl
// ```

use color_eyre::eyre;
use std::collections::HashSet;

use alloy::primitives::Address;
use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use alloy::sol_types::SolEvent;
use alloy_ext::prelude::*;

const PRINT_EVENTS: bool = false;

sol!(
    #[sol(all_derives = true)]
    #[allow(missing_docs)]
    LabNFT,
    "./examples/abi/LabNFT.json"
);

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

    // Note: no burn function
    // https://github.com/moleculeprotocol/onchainlabs/blob/c69b3774a887906a3a05983d4a410847a189a779/docs/nft/labnft-solady-migration-plan.md?plain=1#L704

    let mut logs = Vec::new();
    provider
        .get_logs_ext(
            vec![config.labnft_address],
            HashSet::from([LabNFT::OclTransfer::SIGNATURE_HASH]),
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

    for log in &logs {
        let block = log.block_number.unwrap();

        if log.event_signature_hash() != LabNFT::OclTransfer::SIGNATURE_HASH {
            unreachable!();
        }

        let event = LabNFT::OclTransfer::decode_log(&log.inner)?;

        if PRINT_EVENTS {
            println!("OclTransfer (block: {block}): {event:#?}\n");
        }
    }

    Ok(())
}

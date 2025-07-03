use std::process::ExitCode;

use kamu_molecule_bridge::prelude::*;

fn main() -> ExitCode {
    // TODO: rustls initialize stuff
    // TODO: read config
    // TODO: logging

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(main_async())
}

async fn main_async() -> ExitCode {
    let app = App {};

    match app.run().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, error_dbg = ?e, "Error running application");
            ExitCode::FAILURE
        }
    }
}

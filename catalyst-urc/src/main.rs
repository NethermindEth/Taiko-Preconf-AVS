use anyhow::Error;
use catalyst_common::utils as common_utils;
use tracing::info;

mod l1;
mod registration;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Error> {
    common_utils::logging::init_logging();

    info!("🚀 Starting URC Node v{}", env!("CARGO_PKG_VERSION"));

    let config = common_utils::config::Config::<utils::config::Config>::read_env_variables();

    Ok(())
}

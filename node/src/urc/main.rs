use anyhow::Error;
use catalyst_node::utils as common_utils;
use tracing::info;

mod l1;
mod registration;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Error> {
    common_utils::logging::init_logging();

    info!("🚀 Starting URC Node v{}", env!("CARGO_PKG_VERSION"));

    Ok(())
}

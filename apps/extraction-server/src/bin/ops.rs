use std::io;

use extraction_server::ops::{run_ops, OpsConfig};

#[actix_web::main]
async fn main() -> io::Result<()> {
    let Some(config) = OpsConfig::from_env()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
    else {
        return Ok(());
    };
    run_ops(config).await
}

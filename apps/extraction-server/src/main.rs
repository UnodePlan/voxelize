use std::io;

use extraction_server::{run, ServerConfig};

#[actix_web::main]
async fn main() -> io::Result<()> {
    let config = ServerConfig::from_env()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    run(config).await
}

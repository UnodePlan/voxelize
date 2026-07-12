use std::{env, error::Error};

use extraction_server::persistence::migrate_database;

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = env::var("DATABASE_URL")?;
    migrate_database(&database_url).await?;
    Ok(())
}

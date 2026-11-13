//! Apply ORYN's database migrations and exit. The API also migrates on start; this is
//! for deploy pipelines that run migrations as a separate step.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let pool = oryn_db::connect(&url, 2).await?;
    oryn_db::migrate(&pool).await?;
    println!("migrations applied");
    Ok(())
}

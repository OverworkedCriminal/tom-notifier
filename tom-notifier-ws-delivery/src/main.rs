mod application;

use application::ApplicationEnv;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        // Ignore error becaouse .env file is not required
        // as long as env variables are set.
        let _ = dotenvy::dotenv();
    }

    let env = ApplicationEnv::parse()?;

    application::setup_tracing(&env)?;

    Ok(())
}

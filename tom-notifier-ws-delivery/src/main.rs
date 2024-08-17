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

    tracing::info!("creating application state");
    let (state, state_to_close) = application::create_state(&env).await?;

    tracing::info!("closing application");
    application::close(state_to_close).await;

    Ok(())
}

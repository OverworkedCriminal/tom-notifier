mod application;

use application::ApplicationEnv;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        // Ignore error because .env file is not required
        // as long as env variables are set
        let _ = dotenvy::dotenv();
    }

    let env = ApplicationEnv::parse()?;

    application::setup_tracing(&env)?;

    tracing::info!("creating application state");
    let state = application::create_state(&env);

    tracing::info!("creating application");
    let app = application::create_application(state);

    tracing::info!(address = %env.bind_address, "starting listener");
    let listener = tokio::net::TcpListener::bind(env.bind_address).await?;

    tracing::info!("server started");
    axum::serve(listener, app).await?;

    Ok(())
}

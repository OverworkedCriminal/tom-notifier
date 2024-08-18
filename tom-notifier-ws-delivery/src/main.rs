mod application;
mod auth;
mod dto;
mod error;
mod repository;
mod routing;

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

    tracing::info!("creating middleware");
    let middleware = application::create_middleware(&env);

    tracing::info!("creating application");
    let app = application::create_application(state, middleware);

    tracing::info!(address = %env.bind_address, "starting listener");
    let listener = tokio::net::TcpListener::bind(env.bind_address).await?;

    tracing::info!("server started");
    axum::serve(listener, app)
        .with_graceful_shutdown(application::shutdown_signal())
        .await?;

    tracing::info!("closing application");
    application::close(state_to_close).await;

    tracing::info!("shutdown complete");

    Ok(())
}

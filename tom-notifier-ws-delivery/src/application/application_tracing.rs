use super::ApplicationEnv;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn setup_tracing(env: &ApplicationEnv) -> anyhow::Result<()> {
    let console_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env()?;

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(console_filter);

    let file_appender = tracing_appender::rolling::hourly(&env.log_directory, &env.log_filename);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(false);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    Ok(())
}

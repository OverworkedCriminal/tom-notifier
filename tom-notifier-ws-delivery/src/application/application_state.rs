use super::ApplicationEnv;
use axum::extract::FromRef;

#[derive(Clone, FromRef)]
pub struct ApplicationState {}

pub struct ApplicationStateToClose {}

pub async fn create_state(
    env: &ApplicationEnv,
) -> anyhow::Result<(ApplicationState, ApplicationStateToClose)> {
    Ok((ApplicationState {}, ApplicationStateToClose {}))
}

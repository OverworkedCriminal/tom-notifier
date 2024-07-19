use super::ApplicationEnv;

#[derive(Clone)]
pub struct ApplicationState {}

pub fn create_state(env: &ApplicationEnv) -> ApplicationState {
    ApplicationState {}
}

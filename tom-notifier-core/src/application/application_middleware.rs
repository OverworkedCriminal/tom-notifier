use super::ApplicationEnv;
use crate::auth::JwtAuthorizationValidator;
use tower_http::validate_request::ValidateRequestHeaderLayer;

pub struct ApplicationMiddleware {
    pub auth: ValidateRequestHeaderLayer<JwtAuthorizationValidator>,
}

pub fn create_middleware(env: &ApplicationEnv) -> ApplicationMiddleware {
    let auth = ValidateRequestHeaderLayer::custom(JwtAuthorizationValidator::new(
        env.jwt_key.clone(),
        env.jwt_algorithms.clone(),
    ));

    ApplicationMiddleware { auth }
}

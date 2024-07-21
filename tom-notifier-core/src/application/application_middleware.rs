use super::ApplicationEnv;
use crate::auth::JwtAuthorizationValidator;
use tower_http::{limit::RequestBodyLimitLayer, validate_request::ValidateRequestHeaderLayer};

pub struct ApplicationMiddleware {
    pub auth: ValidateRequestHeaderLayer<JwtAuthorizationValidator>,
    pub body_limit: RequestBodyLimitLayer,
}

pub fn create_middleware(env: &ApplicationEnv) -> ApplicationMiddleware {
    let auth = ValidateRequestHeaderLayer::custom(JwtAuthorizationValidator::new(
        env.jwt_key.clone(),
        env.jwt_algorithms.clone(),
    ));

    let body_limit = RequestBodyLimitLayer::new(env.max_http_content_len);

    ApplicationMiddleware { auth, body_limit }
}

use super::ApplicationEnv;
use crate::auth::JwtAuthorizationValidator;
use axum::http::Request;
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    limit::RequestBodyLimitLayer,
    trace::{MakeSpan, TraceLayer},
    validate_request::ValidateRequestHeaderLayer,
};
use uuid::Uuid;

pub struct ApplicationMiddleware {
    pub auth: ValidateRequestHeaderLayer<JwtAuthorizationValidator>,
    pub body_limit: RequestBodyLimitLayer,
    pub trace: TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MyMakeSpan>,
}

pub fn create_middleware(env: &ApplicationEnv) -> ApplicationMiddleware {
    let auth = ValidateRequestHeaderLayer::custom(JwtAuthorizationValidator::new(
        env.jwt_key.clone(),
        env.jwt_algorithms.clone(),
    ));

    let body_limit = RequestBodyLimitLayer::new(env.max_http_content_len);

    let trace = TraceLayer::new_for_http().make_span_with(MyMakeSpan);

    ApplicationMiddleware {
        auth,
        body_limit,
        trace,
    }
}

#[derive(Clone)]
pub struct MyMakeSpan;
impl<B> MakeSpan<B> for MyMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> tracing::Span {
        let request_id = Uuid::new_v4();
        tracing::info_span!(
            "request",
            %request_id,
            method=%request.method(),
            uri = %request.uri(),
            user_id = tracing::field::Empty
        )
    }
}

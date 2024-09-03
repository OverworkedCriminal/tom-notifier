use super::ApplicationEnv;
use axum::http::Request;
use jwt_auth::middleware::JwtAuthLayer;
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{MakeSpan, TraceLayer},
};
use uuid::Uuid;

pub struct ApplicationMiddleware {
    pub auth: JwtAuthLayer,
    pub trace: TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MyMakeSpan>,
}

pub fn create_middleware(env: &ApplicationEnv) -> ApplicationMiddleware {
    let auth = JwtAuthLayer::new(env.jwt_key.clone(), env.jwt_algorithms.clone());
    let trace = TraceLayer::new_for_http().make_span_with(MyMakeSpan);

    ApplicationMiddleware { auth, trace }
}

#[derive(Clone)]
pub struct MyMakeSpan;
impl<B> MakeSpan<B> for MyMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> tracing::Span {
        let request_id = Uuid::new_v4();
        tracing::info_span!(
            "Request",
            %request_id,
            method=%request.method(),
            uri = %request.uri(),
        )
    }
}

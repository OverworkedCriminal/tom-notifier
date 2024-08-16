use super::jwt_auth_service::JwtAuthService;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use std::sync::Arc;
use tower::Layer;

#[derive(Clone)]
pub struct JwtAuthLayer {
    validation: Arc<Validation>,
    key: Arc<DecodingKey>,
}

impl JwtAuthLayer {
    pub fn new(key: DecodingKey, algorithms: Vec<Algorithm>) -> Self {
        let mut validation = Validation::default();
        validation.algorithms = algorithms;

        Self {
            validation: Arc::new(validation),
            key: Arc::new(key),
        }
    }
}

impl<S> Layer<S> for JwtAuthLayer {
    type Service = JwtAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        JwtAuthService::new(inner, self.validation.clone(), self.key.clone())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::auth::dto::User;
    use axum::{
        body::Body,
        http::{header::AUTHORIZATION, HeaderValue, Method, Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use jsonwebtoken::Algorithm;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn missing_authorization_header() {
        test_invalid_authorization_header(None).await;
    }

    #[tokio::test]
    async fn invalid_authorization_header() {
        test_invalid_authorization_header("invalid characters ąćś").await;
    }

    #[tokio::test]
    async fn authorization_type_not_bearer() {
        test_invalid_authorization_header("NotBearer").await;
    }

    #[tokio::test]
    async fn invalid_token() {
        test_invalid_authorization_header("Bearer that's not correct JWT").await;
    }

    #[tokio::test]
    async fn expired_token() {
        // Generated with JWT.io
        // exp: 12312 (01.01.1970 04:25:12 GMT+0100)
        // key: some secret
        let authorization = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIzNzlhNzNlNi05MWRkLTQ4YTMtYTY1Mi0wMDJkMzRjNDM2NzAiLCJleHAiOjEyMzEyLCJyZWFsbV9hY2Nlc3MiOnsicm9sZXMiOltdfX0.SGmNz26S7UxpQSh8BFm6jQPR5uqrFpafns2NXzcGUT4";
        test_invalid_authorization_header(authorization).await;
    }

    #[tokio::test]
    async fn invalid_signature() {
        // Generated with JWT.io
        // exp: 253402210800 (31.12.9999 00:00:00 GTM+0100)
        // key: Wrong key
        let authorization = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIzNzlhNzNlNi05MWRkLTQ4YTMtYTY1Mi0wMDJkMzRjNDM2NzAiLCJleHAiOjI1MzQwMjIxMDgwMCwicmVhbG1fYWNjZXNzIjp7InJvbGVzIjpbXX19.FR7QPgMA5jes4qjkFEtWGVzc1_lZ-Pidabx3YsfHjOQ";
        test_invalid_authorization_header(authorization).await;
    }

    #[tokio::test]
    async fn correct_request_extension() {
        // Generated with JWT.io
        // exp: 253402210800 (31.12.9999 00:00:00 GTM+0100)
        // sub: "379a73e6-91dd-48a3-a652-002d34c43670"
        // roles: ["first_other_application_role", "second_other_application_role"]
        // key: some secret
        let authorization = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIzNzlhNzNlNi05MWRkLTQ4YTMtYTY1Mi0wMDJkMzRjNDM2NzAiLCJleHAiOjI1MzQwMjIxMDgwMCwicmVhbG1fYWNjZXNzIjp7InJvbGVzIjpbImZpcnN0X290aGVyX2FwcGxpY2F0aW9uX3JvbGUiLCJzZWNvbmRfb3RoZXJfYXBwbGljYXRpb25fcm9sZSJdfX0.06NgghpJPXOV0Hw8Xcxwcy8dL6kO_dkeme5dPic9nMw";
        let algorithms = vec![Algorithm::HS256];
        let key = DecodingKey::from_secret(b"some secret");

        let router = Router::new()
            .route(
                "/",
                get(|Extension(user): Extension<User>| async move {
                    if user.id != Uuid::parse_str("379a73e6-91dd-48a3-a652-002d34c43670").unwrap() {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }

                    let expected_roles = vec![
                        "first_other_application_role".to_string(),
                        "second_other_application_role".to_string(),
                    ];
                    if user.roles != expected_roles {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }

                    StatusCode::OK
                }),
            )
            .route_layer(JwtAuthLayer::new(key, algorithms));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .header(AUTHORIZATION, authorization)
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK)
    }

    async fn test_invalid_authorization_header(authorization: impl Into<Option<&str>>) {
        let algorithms = vec![Algorithm::HS256];
        let key = DecodingKey::from_secret(b"some secret");

        let router = Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .route_layer(JwtAuthLayer::new(key, algorithms));

        let mut request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(Body::empty())
            .unwrap();
        if let Some(authorization) = authorization.into() {
            request
                .headers_mut()
                .insert(AUTHORIZATION, HeaderValue::try_from(authorization).unwrap());
        }

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

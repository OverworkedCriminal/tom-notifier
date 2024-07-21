use super::{dto::JwtClaims, User};
use anyhow::anyhow;
use axum::{
    body::Body,
    http::{header::AUTHORIZATION, HeaderValue, Request, Response, StatusCode},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use std::sync::Arc;
use tower_http::validate_request::ValidateRequest;

///
/// Middleware that validates JWT in Authorization header.
/// If Authorization is correct [User] is added to request extensions.
///
#[derive(Clone)]
pub struct JwtAuthorizationValidator {
    inner: Arc<JwtAuthorizationValidatorInner>,
}

struct JwtAuthorizationValidatorInner {
    key: DecodingKey,
    validation: Validation,
}

impl JwtAuthorizationValidator {
    pub fn new(key: DecodingKey, algorithms: Vec<Algorithm>) -> Self {
        let mut validation = Validation::default();
        validation.algorithms = algorithms;

        let inner = JwtAuthorizationValidatorInner { key, validation };

        Self {
            inner: Arc::new(inner),
        }
    }

    fn try_parse_authorization_header(
        &self,
        authorization_header: Option<&HeaderValue>,
    ) -> anyhow::Result<User> {
        let Some(authorization_header) = authorization_header else {
            return Err(anyhow!("missing Authorization header"));
        };
        let Ok(authorization_value) = authorization_header.to_str() else {
            return Err(anyhow!("illegal character in Authorization header"));
        };
        if !authorization_value.starts_with("Bearer") {
            return Err(anyhow!("unsupported autorization type"));
        }
        let Some(token) = authorization_value.get("Bearer ".len()..) else {
            return Err(anyhow!("invalid jwt"));
        };
        let token_data =
            jsonwebtoken::decode::<JwtClaims>(token, &self.inner.key, &self.inner.validation)?;

        Ok(User::new(
            token_data.claims.sub,
            token_data.claims.realm_access.roles,
        ))
    }
}

impl<B> ValidateRequest<B> for JwtAuthorizationValidator {
    type ResponseBody = Body;

    fn validate(&mut self, request: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
        let authorization_header = request.headers().get(AUTHORIZATION);

        let user = self
            .try_parse_authorization_header(authorization_header)
            .map_err(|err| {
                tracing::warn!(%err, "auth error");
                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Body::empty())
                    .unwrap()
            })?;

        request.extensions_mut().insert(user);

        Ok(())
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
    use tower_http::validate_request::ValidateRequestHeaderLayer;
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
            .route_layer(ValidateRequestHeaderLayer::custom(
                JwtAuthorizationValidator::new(key, algorithms),
            ));

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
            .route_layer(ValidateRequestHeaderLayer::custom(
                JwtAuthorizationValidator::new(key, algorithms),
            ));

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

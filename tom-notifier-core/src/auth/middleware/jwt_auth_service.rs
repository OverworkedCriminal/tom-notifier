use super::jwt_auth_future::JwtAuthFuture;
use crate::auth::dto::{JwtClaims, User};
use anyhow::anyhow;
use axum::{
    extract::Request,
    http::{header::AUTHORIZATION, HeaderValue},
    response::Response,
};
use jsonwebtoken::{DecodingKey, Validation};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

#[derive(Clone)]
pub struct JwtAuthService<S> {
    inner: S,
    validation: Arc<Validation>,
    key: Arc<DecodingKey>,
}

impl<S> JwtAuthService<S> {
    pub fn new(inner: S, validation: Arc<Validation>, key: Arc<DecodingKey>) -> Self {
        Self {
            inner,
            validation,
            key,
        }
    }
}

impl<S> JwtAuthService<S> {
    fn parse_authorization_header(
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
        let token_data = jsonwebtoken::decode::<JwtClaims>(token, &self.key, &self.validation)
            .map_err(|err| anyhow!("invalid jwt: {err}"))?;

        Ok(User::new(
            token_data.claims.sub,
            token_data.claims.realm_access.roles,
        ))
    }
}

impl<S> Service<Request> for JwtAuthService<S>
where
    S: Service<Request, Response = Response>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = JwtAuthFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let authorization_header = req.headers().get(AUTHORIZATION);

        match self.parse_authorization_header(authorization_header) {
            Ok(user) => {
                // crate span that holds user information
                let span = tracing::info_span!("user", id = %user.id);

                req.extensions_mut().insert(user);

                JwtAuthFuture::Authorized {
                    inner: self.inner.call(req),
                    span,
                }
            }
            Err(err) => {
                tracing::warn!(%err, "auth error");
                JwtAuthFuture::Unauthorized
            }
        }
    }
}

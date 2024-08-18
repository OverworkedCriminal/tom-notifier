use crate::{
    application::{ApplicationMiddleware, ApplicationState},
    auth::Role,
    dto::{input, output},
    error::Error,
};
use axum::{
    extract::{Path, Query, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
    routing::{delete, get},
    Extension, Json, Router,
};
use jwt_auth::{functions::require_all_roles, User};
use uuid::Uuid;

pub fn routing(application_middleware: &ApplicationMiddleware) -> Router<ApplicationState> {
    Router::new()
        .route("/api/v1/ticket", get(get_ticket))
        .route("/api/v1/connection/:user_id", delete(delete_connection))
        .route_layer(application_middleware.auth.clone())
        .route("/ws/v1", get(websocket_upgrade))
}

async fn get_ticket(
    Extension(user): Extension<User>,
) -> Result<(StatusCode, Json<output::WebSocketTicket>), Error> {
    todo!("not implemented")
}

async fn delete_connection(
    Extension(user): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, Error> {
    require_all_roles(&user, &[Role::Admin.as_ref()])?;
    todo!("not implemented")
}

async fn websocket_upgrade(
    Query(ticket): Query<input::WebSocketTicket>,
    ws: WebSocketUpgrade,
) -> Result<Response, Error> {
    todo!("not implemented")
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::application::{self, ApplicationEnv};
    use axum::{body::Body, http::Request};
    use http::{header::AUTHORIZATION, Method};
    use jwt_auth::test::create_jwt;
    use std::sync::Once;
    use tower::ServiceExt;

    static BEFORE_ALL: Once = Once::new();

    fn init_env_variables() {
        let _ = dotenvy::dotenv();
    }

    fn mock_application_state() -> ApplicationState {
        ApplicationState {}
    }

    fn routing() -> Router<ApplicationState> {
        let env = ApplicationEnv::parse().unwrap();
        let middleware = application::create_middleware(&env);

        super::routing(&middleware)
    }

    fn create_user_bearer() -> String {
        let jwt_algorithms = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS").unwrap();
        let jwt_key = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_TEST_ENCODE_KEY").unwrap();

        let jwt = create_jwt(Uuid::new_v4(), &[], jwt_algorithms, jwt_key);

        format!("Bearer {jwt}")
    }

    #[tokio::test]
    async fn delete_connection_403() {
        BEFORE_ALL.call_once(init_env_variables);

        let application_state = mock_application_state();

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/v1/connection/{}", Uuid::new_v4()))
                    .header(AUTHORIZATION, create_user_bearer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

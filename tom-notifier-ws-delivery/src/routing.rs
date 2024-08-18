use crate::{
    application::{ApplicationMiddleware, ApplicationState},
    auth::Role,
    dto::{input, output},
    error::Error,
    service::tickets_service::TicketsSerivce,
};
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
    routing::{delete, get},
    Extension, Json, Router,
};
use jwt_auth::{functions::require_all_roles, User};
use std::sync::Arc;
use uuid::Uuid;

pub fn routing(application_middleware: &ApplicationMiddleware) -> Router<ApplicationState> {
    Router::new()
        .route("/api/v1/ticket", get(get_ticket))
        .route("/api/v1/connection/:user_id", delete(delete_connection))
        .route_layer(application_middleware.auth.clone())
        .route("/ws/v1", get(websocket_upgrade))
}

///
/// Create ticket that can be used to establish WebSocket connection with the server
///
/// ### Returns
/// 200 on success
///
async fn get_ticket(
    State(tickets_service): State<Arc<dyn TicketsSerivce>>,
    Extension(user): Extension<User>,
) -> Result<(StatusCode, Json<output::WebSocketTicket>), Error> {
    let ticket = tickets_service.create_ticket(user.id).await?;
    Ok((StatusCode::OK, Json(ticket)))
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
    use crate::{
        application::{self, ApplicationEnv},
        repository,
        service::tickets_service::MockTicketsSerivce,
    };
    use axum::{body::Body, http::Request};
    use http::{header::AUTHORIZATION, Method};
    use jwt_auth::test::create_jwt;
    use std::sync::{Arc, Once};
    use tower::ServiceExt;

    static BEFORE_ALL: Once = Once::new();

    fn init_env_variables() {
        let _ = dotenvy::dotenv();
    }

    fn mock_application_state() -> ApplicationState {
        ApplicationState {
            tickets_service: Arc::new(MockTicketsSerivce::new()),
        }
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
    async fn get_ticket_200() {
        BEFORE_ALL.call_once(init_env_variables);

        let mut tickets_service = MockTicketsSerivce::new();
        tickets_service.expect_create_ticket().returning(|_| {
            Ok(output::WebSocketTicket {
                ticket: "some ticket".to_string(),
            })
        });

        let mut application_state = mock_application_state();
        application_state.tickets_service = Arc::new(tickets_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/ticket")
                    .header(AUTHORIZATION, create_user_bearer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_ticket_database_error() {
        BEFORE_ALL.call_once(init_env_variables);

        let mut tickets_service = MockTicketsSerivce::new();
        tickets_service.expect_create_ticket().returning(|_| {
            Err(Error::Database(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            )))
        });

        let mut application_state = mock_application_state();
        application_state.tickets_service = Arc::new(tickets_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/ticket")
                    .header(AUTHORIZATION, create_user_bearer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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

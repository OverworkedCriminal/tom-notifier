use crate::{
    application::{ApplicationMiddleware, ApplicationState},
    auth::Role,
    dto::{input, output},
    error::Error,
    service::{tickets_service::TicketsSerivce, websockets_service::WebSocketsService},
};
use axum::{
    extract::{ConnectInfo, Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
    routing::{delete, get},
    Extension, Json, Router,
};
use jwt_auth::{functions::require_all_roles, User};
use std::{net::SocketAddr, sync::Arc};
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

///
/// Close all connections bound to passed user_id
///
/// ### Returns
/// 204 on success
///
/// ### Errors
/// - 403 when
///     - user using this endpoint lacks role [Role::Admin]
///
async fn delete_connection(
    State(websockets_service): State<Arc<dyn WebSocketsService>>,
    Extension(user): Extension<User>,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, Error> {
    require_all_roles(&user, &[Role::Admin.as_ref()])?;
    websockets_service.close_connections(user_id).await;
    Ok(StatusCode::NO_CONTENT)
}

///
/// Start WebSocket connection
///
/// ### Errors
/// - 401 when
///     - ticket does not exist
///     - ticket has already been used
///     - ticket has expired
///
async fn websocket_upgrade(
    State(tickets_service): State<Arc<dyn TicketsSerivce>>,
    State(websockets_service): State<Arc<dyn WebSocketsService>>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Query(ticket): Query<input::WebSocketTicket>,
    websocket_upgrade: WebSocketUpgrade,
) -> Result<Response, Error> {
    let ticket = tickets_service.consume_ticket(ticket).await?;
    let response = websocket_upgrade.on_upgrade(move |websocket| async move {
        websockets_service
            .handle_client(ticket.user_id, address, websocket)
            .await;
    });
    Ok(response)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        application::{self, ApplicationEnv},
        repository,
        service::{tickets_service::MockTicketsSerivce, websockets_service::MockWebSocketsService},
    };
    use axum::{body::Body, http::Request};
    use http::{header::AUTHORIZATION, Method};
    use jwt_auth::test::create_jwt;
    use std::{
        future::IntoFuture,
        net::{Ipv4Addr, SocketAddr},
        sync::{Arc, Once},
    };
    use tokio::{net::TcpListener, sync::Notify};
    use tower::ServiceExt;

    static BEFORE_ALL: Once = Once::new();

    fn init_env_variables() {
        let _ = dotenvy::dotenv();
    }

    fn mock_application_state() -> ApplicationState {
        ApplicationState {
            tickets_service: Arc::new(MockTicketsSerivce::new()),
            websockets_service: Arc::new(MockWebSocketsService::new()),
        }
    }

    fn routing() -> Router<ApplicationState> {
        let env = ApplicationEnv::parse().unwrap();
        let middleware = application::create_middleware(&env);

        super::routing(&middleware)
    }

    fn create_user_bearer() -> String {
        create_bearer(&[])
    }

    fn create_admin_bearer() -> String {
        create_bearer(&[Role::Admin.as_ref()])
    }

    fn create_bearer(roles: &[&str]) -> String {
        let jwt_algorithms = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS").unwrap();
        let jwt_key = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_TEST_ENCODE_KEY").unwrap();

        let jwt = create_jwt(Uuid::new_v4(), roles, jwt_algorithms, jwt_key);

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

    #[tokio::test]
    async fn delete_connection_204() {
        BEFORE_ALL.call_once(init_env_variables);

        let mut websockets_service = MockWebSocketsService::new();
        websockets_service
            .expect_close_connections()
            .returning(|_| ());
        let mut application_state = mock_application_state();
        application_state.websockets_service = Arc::new(websockets_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/v1/connection/{}", Uuid::new_v4()))
                    .header(AUTHORIZATION, create_admin_bearer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn websocket_upgrade_ticket_invalid() {
        let mut tickets_service = MockTicketsSerivce::new();
        tickets_service
            .expect_consume_ticket()
            .returning(|_| Err(Error::TicketInvalid("ticket is invalid")));

        let mut application_state = mock_application_state();
        application_state.tickets_service = Arc::new(tickets_service);

        let app = routing()
            .with_state(application_state)
            .into_make_service_with_connect_info::<SocketAddr>();

        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();

        let shutdown_notify = Arc::new(Notify::new());
        let shutdown_notify_clone = Arc::clone(&shutdown_notify);

        let server_handle = tokio::spawn(
            axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown_notify_clone.notified().await })
                .into_future(),
        );

        let connect_err = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/ws/v1?ticket=anystringwilldohere"
        ))
        .await
        .unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = connect_err else {
            panic!("unexpected error");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        shutdown_notify.notify_one();
        server_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn websocket_upgrade_ticket_consume_database_error() {
        let mut tickets_service = MockTicketsSerivce::new();
        tickets_service.expect_consume_ticket().returning(|_| {
            Err(Error::Database(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            )))
        });

        let mut application_state = mock_application_state();
        application_state.tickets_service = Arc::new(tickets_service);

        let app = routing()
            .with_state(application_state)
            .into_make_service_with_connect_info::<SocketAddr>();

        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();

        let shutdown_notify = Arc::new(Notify::new());
        let shutdown_notify_clone = Arc::clone(&shutdown_notify);

        let server_handle = tokio::spawn(
            axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown_notify_clone.notified().await })
                .into_future(),
        );

        let connect_err = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/ws/v1?ticket=anystringwilldohere"
        ))
        .await
        .unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = connect_err else {
            panic!("unexpected error");
        };
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        shutdown_notify.notify_one();
        server_handle.await.unwrap().unwrap();
    }
}

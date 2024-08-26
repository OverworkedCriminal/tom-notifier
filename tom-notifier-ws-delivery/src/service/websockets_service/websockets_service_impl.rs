use super::{
    dto::{WebSocketMessage, WebSocketsServiceConfig},
    websocket_confirmation_callback::WebSocketConfirmationCallback,
    WebSocketsService,
};
use crate::{
    dto::output,
    service::{
        confirmations_service::ConfirmationsService,
        websockets_service::websocket_connection::WebSocketConnection,
    },
};
use axum::{async_trait, extract::ws::WebSocket};
use futures::StreamExt;
use prost::Message as ProstMessage;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

pub struct WebSocketsServiceImpl {
    config: Arc<WebSocketsServiceConfig>,

    network_status_ok: AtomicBool,
    users_connections: Arc<RwLock<HashMap<Uuid, broadcast::Sender<Arc<WebSocketMessage>>>>>,

    confirmations_service: Arc<dyn ConfirmationsService>,
}

impl WebSocketsServiceImpl {
    pub fn new(
        config: WebSocketsServiceConfig,
        confirmations_service: Arc<dyn ConfirmationsService>,
    ) -> Self {
        let users_connections = HashMap::new();
        let users_connections = RwLock::new(users_connections);
        let users_connections = Arc::new(users_connections);

        Self {
            config: Arc::new(config),
            network_status_ok: AtomicBool::new(true),
            users_connections,
            confirmations_service,
        }
    }

    fn create_message(
        &self,
        network_status: output::NetworkStatusProtobuf,
        notification: Option<output::NotificationProtobuf>,
    ) -> Arc<WebSocketMessage> {
        let message_id = Uuid::new_v4();
        let delivered_callback = notification
            .as_ref()
            .filter(|notification| notification.status() == output::NotificationStatusProtobuf::New)
            .map(|notification| {
                WebSocketConfirmationCallback::new(
                    Arc::clone(&self.confirmations_service),
                    notification.id.clone(),
                )
            });

        let websocket_message = output::WebSocketNotificationProtobuf {
            message_id: message_id.to_string(),
            network_status: network_status.into(),
            notification,
        };

        let payload = websocket_message.encode_to_vec();

        Arc::new(WebSocketMessage {
            message_id,
            payload,
            delivered_callback,
        })
    }

    async fn send_multicast(&self, user_ids: &[Uuid], message: Arc<WebSocketMessage>) {
        let connections = self.users_connections.read().await;
        user_ids
            .into_iter()
            .filter_map(|user_id| connections.get_key_value(user_id))
            .for_each(|(user_id, tx)| {
                let _ = tx.send(message.clone());
                tracing::info!(
                    message_id = message.message_id.to_string(),
                    %user_id,
                    "queued message to be sent",
                );
            });
    }

    async fn send_broadcast(&self, message: Arc<WebSocketMessage>) {
        let connections = self.users_connections.read().await;
        connections.iter().for_each(|(user_id, tx)| {
            let _ = tx.send(message.clone());
            tracing::info!(
                message_id = message.message_id.to_string(),
                %user_id,
                "queued message to be sent",
            );
        });
    }
}

#[async_trait]
impl WebSocketsService for WebSocketsServiceImpl {
    async fn handle_client(&self, user_id: Uuid, address: SocketAddr, websocket: WebSocket) {
        let user_id_str = user_id.to_string();
        let address_str = address.to_string();

        tracing::info!(
            user_id = user_id_str,
            address = address_str,
            "starting websocket connection",
        );

        // Find messages channel or create new one if necessary
        let (messages_tx, messages_rx) = {
            let mut connections_lock = self.users_connections.write().await;
            match connections_lock.get(&user_id) {
                Some(messages_tx) => (messages_tx.clone(), messages_tx.subscribe()),
                None => {
                    let (messages_tx, messages_rx) = tokio::sync::broadcast::channel(16);
                    connections_lock.insert(user_id, messages_tx.clone());
                    tracing::trace!(user_id = user_id_str, "added user to connected_users");
                    (messages_tx, messages_rx)
                }
            }
        };

        // If there's a network problem, user should be informed that he
        // should not rely on websockets until problem is gone
        let network_status_ok = self.network_status_ok.load(Ordering::Acquire);
        if !network_status_ok {
            let message = self.create_message(output::NetworkStatusProtobuf::Error, None);
            unsafe { messages_tx.send(message).unwrap_unchecked() };
        }

        // Create connection
        let (ws_tx, ws_rx) = websocket.split();
        let connection = WebSocketConnection::new(
            Arc::clone(&self.config),
            user_id,
            address,
            messages_rx,
            ws_tx,
            ws_rx,
        );

        let users_connections = Arc::clone(&self.users_connections);

        // Run connection
        tokio::spawn(async move {
            tracing::info!(
                user_id = user_id_str,
                address = address_str,
                "websocket connection started"
            );

            connection.run().await;

            let mut lock = users_connections.write().await;
            if let Some(tx) = lock.get(&user_id) {
                if tx.receiver_count() == 0 {
                    lock.remove(&user_id);
                    tracing::trace!(user_id = user_id_str, "removed user from user_connections");
                }
            }

            tracing::info!(
                user_id = user_id_str,
                address = address_str,
                "websocket connection finished"
            );
        });
    }

    async fn close_connections(&self, user_id: Uuid) {
        let count = {
            let mut connections_lock = self.users_connections.write().await;
            connections_lock.remove(&user_id)
        }
        .map(|tx| tx.receiver_count())
        .unwrap_or(0);

        tracing::info!(%user_id, count, "closing user connections");
    }

    async fn send(&self, user_ids: &[Uuid], notification: output::NotificationProtobuf) {
        let message = self.create_message(output::NetworkStatusProtobuf::Ok, Some(notification));
        match user_ids.is_empty() {
            true => self.send_broadcast(message).await,
            false => self.send_multicast(user_ids, message).await,
        }
    }

    async fn update_network_status(&self, status: output::NetworkStatusProtobuf) {
        // Store network status so incomming connections can be informed
        // about network error
        self.network_status_ok.store(
            status == output::NetworkStatusProtobuf::Ok,
            Ordering::Release,
        );

        // Send information about network problems to every connected user
        // so they can start using long polling
        let message = self.create_message(status, None);
        self.send_broadcast(message).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service::confirmations_service::MockConfirmationsService;
    use broadcast::error::RecvError;
    use prost_types::Timestamp;
    use std::time::Duration;
    use time::OffsetDateTime;

    #[tokio::test]
    async fn close_connections_channel_gets_closed() {
        let service = create_service();
        let user_id = Uuid::new_v4();

        // simulate connection
        let (tx, mut rx) = broadcast::channel(8);
        let mut lock = service.users_connections.write().await;
        lock.insert(user_id, tx);
        drop(lock);

        service.close_connections(user_id).await;

        let message = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap();

        assert!(matches!(message, Err(RecvError::Closed)));
    }

    #[tokio::test]
    async fn send_unicast_correct_channel_received_message() {
        let service = create_service();
        let user_1_id = Uuid::new_v4();
        let user_2_id = Uuid::new_v4();

        // simulate connection
        let (tx_1, mut rx_1) = broadcast::channel(8);
        let (tx_2, mut rx_2) = broadcast::channel(8);
        let mut lock = service.users_connections.write().await;
        lock.insert(user_1_id, tx_1);
        lock.insert(user_2_id, tx_2);
        drop(lock);

        service.send(&[user_1_id], create_notification()).await;

        let (t1, t2) = tokio::join!(
            tokio::time::timeout(Duration::from_millis(100), rx_1.recv()),
            tokio::time::timeout(Duration::from_millis(100), rx_2.recv()),
        );

        assert!(t1.is_ok());
        assert!(t2.is_err());
    }

    #[tokio::test]
    async fn send_multicast_correct_channels_received_message() {
        let service = create_service();
        let user_1_id = Uuid::new_v4();
        let user_2_id = Uuid::new_v4();
        let user_3_id = Uuid::new_v4();

        // simulate connection
        let (tx_1, mut rx_1) = broadcast::channel(8);
        let (tx_2, mut rx_2) = broadcast::channel(8);
        let (tx_3, mut rx_3) = broadcast::channel(8);
        let mut lock = service.users_connections.write().await;
        lock.insert(user_1_id, tx_1);
        lock.insert(user_2_id, tx_2);
        lock.insert(user_3_id, tx_3);
        drop(lock);

        service
            .send(&[user_1_id, user_3_id], create_notification())
            .await;

        let (t1, t2, t3) = tokio::join!(
            tokio::time::timeout(Duration::from_millis(100), rx_1.recv()),
            tokio::time::timeout(Duration::from_millis(100), rx_2.recv()),
            tokio::time::timeout(Duration::from_millis(100), rx_3.recv()),
        );

        assert!(t1.is_ok());
        assert!(t2.is_err());
        assert!(t3.is_ok());
    }

    #[tokio::test]
    async fn send_broadcast_all_channels_received_message() {
        let service = create_service();
        let user_1_id = Uuid::new_v4();
        let user_2_id = Uuid::new_v4();
        let user_3_id = Uuid::new_v4();

        // simulate connection
        let (tx_1, mut rx_1) = broadcast::channel(8);
        let (tx_2, mut rx_2) = broadcast::channel(8);
        let (tx_3, mut rx_3) = broadcast::channel(8);
        let mut lock = service.users_connections.write().await;
        lock.insert(user_1_id, tx_1);
        lock.insert(user_2_id, tx_2);
        lock.insert(user_3_id, tx_3);
        drop(lock);

        service.send(&[], create_notification()).await;

        let (t1, t2, t3) = tokio::join!(
            tokio::time::timeout(Duration::from_millis(100), rx_1.recv()),
            tokio::time::timeout(Duration::from_millis(100), rx_2.recv()),
            tokio::time::timeout(Duration::from_millis(100), rx_3.recv()),
        );

        assert!(t1.is_ok());
        assert!(t2.is_ok());
        assert!(t3.is_ok());
    }

    #[tokio::test]
    async fn send_new_callback_present() {
        let service = create_service();
        let user_id = Uuid::new_v4();

        // simulate connection
        let (tx, mut rx) = broadcast::channel(8);
        {
            let mut lock = service.users_connections.write().await;
            lock.insert(user_id, tx);
        }

        let notification = output::NotificationProtobuf {
            id: "any string should be okay".to_string(),
            status: output::NotificationStatusProtobuf::New.into(),
            timestamp: Some(Timestamp {
                seconds: OffsetDateTime::now_utc().unix_timestamp(),
                nanos: 0,
            }),
            created_by: Some("user".to_string()),
            seen: Some(false),
            content_type: Some("content type".to_string()),
            content: Some(b"content".to_vec()),
        };

        service.send(&[user_id], notification).await;

        let message = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(message.delivered_callback.is_some());
    }

    #[tokio::test]
    async fn send_updated_callback_not_present() {
        let service = create_service();
        let user_id = Uuid::new_v4();

        // simulate connection
        let (tx, mut rx) = broadcast::channel(8);
        {
            let mut lock = service.users_connections.write().await;
            lock.insert(user_id, tx);
        }

        let notification = output::NotificationProtobuf {
            id: "any string should be okay".to_string(),
            status: output::NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: OffsetDateTime::now_utc().unix_timestamp(),
                nanos: 0,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        };

        service.send(&[user_id], notification).await;

        let message = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(message.delivered_callback.is_none());
    }

    #[tokio::test]
    async fn send_deleted_callback_not_present() {
        let service = create_service();
        let user_id = Uuid::new_v4();

        // simulate connection
        let (tx, mut rx) = broadcast::channel(8);
        {
            let mut lock = service.users_connections.write().await;
            lock.insert(user_id, tx);
        }

        let notification = output::NotificationProtobuf {
            id: "any string should be okay".to_string(),
            status: output::NotificationStatusProtobuf::Deleted.into(),
            timestamp: Some(Timestamp {
                seconds: OffsetDateTime::now_utc().unix_timestamp(),
                nanos: 0,
            }),
            created_by: None,
            seen: None,
            content_type: None,
            content: None,
        };

        service.send(&[user_id], notification).await;

        let message = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(message.delivered_callback.is_none());
    }

    fn create_service() -> WebSocketsServiceImpl {
        // config does not matter
        let config = WebSocketsServiceConfig {
            ping_interval: Duration::from_secs(600),
            retry_max_count: 10,
            retry_interval: Duration::from_secs(10),
        };

        WebSocketsServiceImpl::new(config, Arc::new(MockConfirmationsService::new()))
    }

    fn create_notification() -> output::NotificationProtobuf {
        output::NotificationProtobuf {
            id: "any string should be okay".to_string(),
            status: output::NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: OffsetDateTime::now_utc().unix_timestamp(),
                nanos: 0,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }
    }
}

use super::{dto::Message, WebSocketsService};
use crate::{
    dto::{input::NotificationStatusProtobuf, output},
    service::confirmations_service::ConfirmationsService,
};
use axum::{async_trait, extract::ws::WebSocket};
use prost::Message as ProstMessage;
use std::{collections::HashMap, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

pub struct WebSocketsServiceImpl {
    users_connections: Arc<RwLock<HashMap<Uuid, broadcast::Sender<Arc<Message>>>>>,

    confirmations_service: Arc<dyn ConfirmationsService>,
}

impl WebSocketsServiceImpl {
    pub fn new(confirmations_service: Arc<dyn ConfirmationsService>) -> Self {
        let users_connections = HashMap::new();
        let users_connections = RwLock::new(users_connections);
        let users_connections = Arc::new(users_connections);

        Self {
            users_connections,
            confirmations_service,
        }
    }

    fn create_message(&self, notification: output::NotificationProtobuf) -> Arc<Message> {
        let message_id = Uuid::new_v4();
        let payload = notification.encode_to_vec();
        let delivered_callback: Option<
            Box<dyn FnOnce(Uuid) -> Pin<Box<dyn Future<Output = ()>>> + Send + Sync>,
        > = match notification.status() == NotificationStatusProtobuf::New {
            true => {
                let id = notification.id;
                let confirmations_service = Arc::clone(&self.confirmations_service);
                Some(Box::new(|user_id| {
                    Box::pin(async move {
                        let confirmation = output::ConfirmationProtobuf {
                            id,
                            user_id: user_id.to_string(),
                        };
                        confirmations_service.send(confirmation).await;
                    })
                }))
            }
            false => None,
        };

        Arc::new(Message {
            message_id,
            payload,
            delivered_callback,
        })
    }

    async fn send_multicast(&self, user_ids: &[Uuid], message: Arc<Message>) {
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

    async fn send_broadcast(&self, message: Arc<Message>) {
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
        let message = self.create_message(notification);
        match user_ids.is_empty() {
            true => self.send_broadcast(message).await,
            false => self.send_multicast(user_ids, message).await,
        }
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

    fn create_service() -> WebSocketsServiceImpl {
        WebSocketsServiceImpl::new(Arc::new(MockConfirmationsService::new()))
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

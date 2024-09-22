use super::{
    dto::{WebSocketMessage, WebSocketUnconfirmedMessage, WebSocketsServiceConfig},
    error::Error,
};
use crate::dto::input;
use anyhow::anyhow;
use axum::extract::ws::Message;
use futures::{Sink, SinkExt, Stream, StreamExt};
use prost::Message as ProstMessage;
use std::{collections::VecDeque, fmt::Display, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::{
    sync::broadcast,
    time::{sleep_until, Instant},
};
use uuid::Uuid;

pub struct WebSocketConnection<WebSocketSink, WebSocketStream> {
    config: Arc<WebSocketsServiceConfig>,

    user_id: Uuid,
    user_address: SocketAddr,

    messages_rx: broadcast::Receiver<Arc<WebSocketMessage>>,
    ws_tx: WebSocketSink,
    ws_rx: WebSocketStream,

    unconfirmed_messages: VecDeque<WebSocketUnconfirmedMessage>,

    ping_time: Instant,
    ping_message: u32,
    pings_sent: u8,
}

impl<WebSocketSink, WebSocketStream, SinkError, StreamError>
    WebSocketConnection<WebSocketSink, WebSocketStream>
where
    WebSocketSink: Sink<Message, Error = SinkError> + Unpin,
    WebSocketStream: Stream<Item = Result<Message, StreamError>> + Unpin,
    SinkError: Display,
    StreamError: Display,
{
    pub fn new(
        config: Arc<WebSocketsServiceConfig>,
        user_id: Uuid,
        user_address: SocketAddr,
        messages_rx: broadcast::Receiver<Arc<WebSocketMessage>>,
        ws_tx: WebSocketSink,
        ws_rx: WebSocketStream,
    ) -> Self {
        let messages_queue = VecDeque::new();
        let ping_time = Instant::now() + config.ping_interval;
        let ping_message = 0;
        let pings_sent = 0;

        Self {
            config,
            user_id,
            user_address,
            messages_rx,
            ws_tx,
            ws_rx,
            unconfirmed_messages: messages_queue,
            ping_time,
            ping_message,
            pings_sent,
        }
    }

    #[tracing::instrument(
        name = "WebSocket",
        skip_all,
        fields(
            user_id = %self.user_id,
            address = %self.user_address,
        )
    )]
    pub async fn run(mut self) {
        match self.try_run().await {
            Ok(()) => (),
            Err(Error::Close(message)) => {
                tracing::info!("closing connection: {message}");
            }
            Err(Error::Anyhow(err)) => {
                tracing::warn!("{err}");
            }
        }

        tracing::info!("closing websocket");
        match self.ws_tx.close().await {
            Ok(()) => tracing::info!("websocket closed"),
            Err(err) => tracing::warn!(%err, "failed to close websocket"),
        }
    }

    async fn try_run(&mut self) -> Result<(), Error> {
        loop {
            // Retry can be set to Instant::now()
            // because sleep_until() is not awaited unless messages_queue is empty
            let retry_time = self
                .unconfirmed_messages
                .front()
                .map(|message| message.retry_at)
                .unwrap_or_else(|| Instant::now());

            tokio::select! {
                biased;

                // Wait for time to send the ping
                _ = sleep_until(self.ping_time) => {
                    self.process_ping().await?;
                }

                // Wait for message from the user
                message = self.ws_rx.next() => {
                    self.process_incomming_message(message).await?;
                }

                // Wait for new message to send
                message = self.messages_rx.recv() => {
                    self.process_message(message).await?;
                }

                // Wait for time to resend unconfirmed message from the queue
                _ = sleep_until(retry_time), if !self.unconfirmed_messages.is_empty() => {
                    let queued = unsafe { self.unconfirmed_messages.pop_front().unwrap_unchecked() };
                    self.process_unconfirmed_message(queued).await?;
                }
            }
        }
    }

    async fn process_ping(&mut self) -> anyhow::Result<()> {
        // If after sending 2 pings none of them is responded with a pong,
        // user is unresponsive and connection should be closed
        if self.pings_sent > 1 {
            anyhow::bail!("user unresponsive");
        }

        // If this is first ping of the heartbeat
        // it should be sent with a new message
        if self.pings_sent == 0 {
            self.ping_message += 1;
        }

        let bytes = self.ping_message.to_be_bytes().to_vec();
        self.ws_tx
            .send(Message::Ping(bytes))
            .await
            .map_err(|err| anyhow!("failed to send ping: {err}"))?;
        tracing::trace!(ping_message = self.ping_message, "ping sent");

        self.pings_sent += 1;
        self.ping_time = Instant::now() + self.config.ping_interval;

        Ok(())
    }

    async fn process_incomming_message(
        &mut self,
        message: Option<Result<Message, StreamError>>,
    ) -> Result<(), Error> {
        match message {
            Some(Ok(Message::Binary(payload))) => {
                tracing::debug!("processing confirmation");
                self.process_incomming_binary_message(payload).await?;
                tracing::debug!("processed confirmation");
            }
            Some(Ok(Message::Text(_))) => {
                return Err(Error::Anyhow(anyhow!("received text message")));
            }
            Some(Ok(Message::Ping(_))) => tracing::trace!("processed ping message"),
            Some(Ok(Message::Pong(payload))) => {
                tracing::trace!("processing pong message");
                self.process_incomming_pong_message(payload)?;
                tracing::trace!("processed pong message");
            }
            Some(Ok(Message::Close(_))) => {
                return Err(Error::Close("received close message"));
            }
            Some(Err(err)) => {
                return Err(Error::Anyhow(anyhow!(
                    "failed to read incomming message: {err}"
                )));
            }
            None => return Err(Error::Anyhow(anyhow!("incomming messages stream closed"))),
        }

        Ok(())
    }

    async fn process_incomming_binary_message(&mut self, payload: Vec<u8>) -> anyhow::Result<()> {
        let confirmation = input::WebSocketConfirmationProtobuf::decode(payload.as_slice())
            .map_err(|err| anyhow!("failed to decode confirmation: {err}"))?;
        let message_id_str = &confirmation.message_id;
        let message_id = Uuid::from_str(&confirmation.message_id)
            .map_err(|err| anyhow!("failed to decode confirmation: invalid message_id: {err}"))?;

        let Some(idx) = self
            .unconfirmed_messages
            .iter()
            .position(|queued| queued.message.message_id == message_id)
        else {
            tracing::debug!(message_id = message_id_str, "confirmation was not expected");
            return Ok(());
        };

        // Since user confirmed message he is responsive, so sending ping can be deferred
        self.ping_time = Instant::now() + self.config.ping_interval;
        self.pings_sent = 0;

        // Since idx is found earlier it is safe to call unwrap here
        let queued = unsafe { self.unconfirmed_messages.remove(idx).unwrap_unchecked() };
        tracing::debug!(message_id = message_id_str, "message confirmed");

        // Execute callback if it is present
        if let Some(callback) = queued.message.delivered_callback.as_ref() {
            callback.execute(self.user_id).await;
        }

        Ok(())
    }

    fn process_incomming_pong_message(&mut self, payload: Vec<u8>) -> anyhow::Result<()> {
        let byte_array = payload.try_into().map_err(|err: Vec<u8>| {
            anyhow!(
                "pong payload length invalid: len {} expected {}",
                err.len(),
                size_of::<u32>()
            )
        })?;
        let pong_message = u32::from_be_bytes(byte_array);

        // Pong was delayed or no ping was sent.
        // Receiving pong in this case is not an error
        if self.pings_sent == 0 {
            tracing::trace!("pong was not expected");
            return Ok(());
        }

        // Pong was delayed and new ping had already been sent.
        // Receiving pong in this case is also not an error
        if pong_message != self.ping_message {
            tracing::trace!(
                pong_message,
                ping_message = self.ping_message,
                "pong message does not match expected message"
            );
            return Ok(());
        }

        // At this point pong_message matches sent ping_message
        // so user is responsisve and ping can be deferred
        self.ping_time = Instant::now() + self.config.ping_interval;
        self.pings_sent = 0;

        Ok(())
    }

    async fn process_message(
        &mut self,
        message: Result<Arc<WebSocketMessage>, broadcast::error::RecvError>,
    ) -> Result<(), Error> {
        match message {
            Err(broadcast::error::RecvError::Lagged(count)) => Err(Error::Anyhow(anyhow!(
                "connection lagged. skipped messages: {count}"
            ))),
            Err(broadcast::error::RecvError::Closed) => {
                Err(Error::Close("connection forcefully closed"))
            }
            Ok(message) => {
                let message_id_str = message.message_id.to_string();

                tracing::info!(message_id = message_id_str, "sending message");
                self.ws_tx
                    .send(Message::Binary(message.payload.clone()))
                    .await
                    .map_err(|err| anyhow!("sending message failed: {err}"))?;

                let message = WebSocketUnconfirmedMessage {
                    retry_at: Instant::now() + self.config.retry_interval,
                    retries_remaining: self.config.retry_max_count,
                    message,
                };
                self.unconfirmed_messages.push_back(message);
                tracing::debug!(
                    message_id = message_id_str,
                    "message waits for confirmation"
                );

                tracing::info!(message_id = message_id_str, "sent message");

                Ok(())
            }
        }
    }

    async fn process_unconfirmed_message(
        &mut self,
        mut unconfirmed: WebSocketUnconfirmedMessage,
    ) -> anyhow::Result<()> {
        let message_id_str = unconfirmed.message.message_id.to_string();

        tracing::debug!(
            message_id = message_id_str,
            "processing unconfirmed message",
        );

        if unconfirmed.retries_remaining == 0 {
            return Err(anyhow!(
                "user unresponsive: message {message_id_str} not confirmed in time"
            ));
        }

        unconfirmed.retry_at = Instant::now() + self.config.retry_interval;
        unconfirmed.retries_remaining -= 1;

        tracing::debug!(
            message_id = message_id_str,
            retries_remaining = unconfirmed.retries_remaining,
            "resending message"
        );
        self.ws_tx
            .send(Message::Binary(unconfirmed.message.payload.clone()))
            .await
            .map_err(|err| anyhow!("failed to resend message: {err}"))?;

        self.unconfirmed_messages.push_back(unconfirmed);

        tracing::debug!(message_id = message_id_str, "processed unconfirmed message");

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::service::{
        confirmations_service::MockConfirmationsService,
        websockets_service::websocket_confirmation_callback::WebSocketConfirmationCallback,
    };
    use std::time::Duration;
    use time::OffsetDateTime;
    use tokio::time::timeout;

    #[tokio::test]
    async fn ping_is_sent_after_interval() {
        let time_begin = OffsetDateTime::now_utc();
        let ping_interval = Duration::from_millis(50);

        let mut config = create_test_config();
        config.ping_interval = ping_interval;

        let (_handle, _ws_tx, mut ws_rx, _notifications_tx) = start_test_connection(config);

        let message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap(); // message

        assert!(matches!(message, Message::Ping(_)));

        let time_now = OffsetDateTime::now_utc();
        assert!(time_now >= time_begin + ping_interval);
    }

    #[tokio::test]
    async fn ping_is_sent_after_second_interval() {
        let time_begin = OffsetDateTime::now_utc();
        let ping_interval = Duration::from_millis(50);

        let mut config = create_test_config();
        config.ping_interval = ping_interval;

        let (_handle, _ws_tx, mut ws_rx, _notifications_tx) = start_test_connection(config);

        for i in 1..=2 {
            let message = timeout(Duration::from_secs(1), ws_rx.next())
                .await
                .unwrap() // timeout
                .unwrap(); // message

            assert!(matches!(message, Message::Ping(_)));

            let time_now = OffsetDateTime::now_utc();
            assert!(time_now >= time_begin + i * ping_interval);
        }
    }

    #[tokio::test]
    async fn ping_is_sent_after_pong_response() {
        let time_begin = OffsetDateTime::now_utc();
        let ping_interval = Duration::from_millis(50);

        let mut config = create_test_config();
        config.ping_interval = ping_interval;

        let (_handle, mut ws_tx, mut ws_rx, _notifications_tx) = start_test_connection(config);

        let message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap(); // message

        let Message::Ping(payload) = message else {
            panic!("invalid message type");
        };

        let time_now = OffsetDateTime::now_utc();
        assert!(time_now >= time_begin + ping_interval);

        // respond with pong
        ws_tx.send(Ok(Message::Pong(payload))).await.unwrap();

        let message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap(); // message
        assert!(matches!(message, Message::Ping(_)));

        let time_now = OffsetDateTime::now_utc();
        assert!(time_now >= time_begin + (ping_interval * 2));
    }

    #[tokio::test]
    async fn ping_user_unresponsive() {
        let time_begin = OffsetDateTime::now_utc();
        let ping_interval = Duration::from_millis(50);

        let mut config = create_test_config();
        config.ping_interval = ping_interval;

        let (handle, _ws_tx, mut ws_rx, _notifications_tx) = start_test_connection(config);

        for i in 1..=2 {
            let message = timeout(Duration::from_secs(1), ws_rx.next())
                .await
                .unwrap() // timeout
                .unwrap(); // message
            assert!(matches!(message, Message::Ping(_)));

            let time_now = OffsetDateTime::now_utc();
            assert!(time_now >= time_begin + i * ping_interval);
        }

        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap(); //handle should never panic

        let time_now = OffsetDateTime::now_utc();
        assert!(time_now >= time_begin + (ping_interval * 3));
    }

    #[tokio::test]
    async fn ping_websocket_connection_closed() {
        let mut config = create_test_config();
        config.ping_interval = Duration::from_millis(50);

        let (handle, _ws_tx, ws_rx, _notifications_tx) = start_test_connection(config);

        // drop websocket to close connection
        drop(ws_rx);

        // handle should finish before timeout
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_confirmation_delivered_callback_executed() {
        let config = create_test_config();

        let mut confirmations_service = MockConfirmationsService::new();
        confirmations_service
            .expect_send()
            .once() // Most important assertion
            .returning(|_| ());

        let (handle, mut ws_tx, mut ws_rx, notifications_tx) = start_test_connection(config);

        let message_id = Uuid::new_v4();
        let message = Arc::new(WebSocketMessage {
            message_id,
            payload: b"payload does not matter".to_vec(),
            delivered_callback: Some(WebSocketConfirmationCallback::new(
                Arc::new(confirmations_service),
                "any string will do".to_string(),
            )),
        });

        let _ = notifications_tx.send(message);

        let message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap(); // message
        let Message::Binary(_) = message else {
            panic!("invalid message type");
        };

        let confirmation = input::WebSocketConfirmationProtobuf {
            message_id: message_id.to_string(),
        };
        let confirmation = input::WebSocketConfirmationProtobuf::encode_to_vec(&confirmation);

        ws_tx.send(Ok(Message::Binary(confirmation))).await.unwrap();

        // assertions in mock objects happen when objects get destroyed.
        // To make sure object get destroyed and panic is propagated
        // to test main thread it's necessary to finish output thread

        // drop channel to finish connection
        drop(ws_tx);

        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap(); // task - mock assertions happen here
    }

    #[tokio::test]
    async fn response_channel_closed() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx.close().await.unwrap();

        // assert thread finished after closing connection
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_invalid_binary_message_not_valid_protobuf() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx.send(Ok(Message::Binary(vec![0x00]))).await.unwrap();

        // assert thread finished after receiving invalid binary message
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_invalid_binary_message_not_valid_message_id() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        let confirmation = input::WebSocketConfirmationProtobuf {
            message_id: "invalid id".to_string(),
        };
        let confirmation = input::WebSocketConfirmationProtobuf::encode_to_vec(&confirmation);

        ws_tx.send(Ok(Message::Binary(confirmation))).await.unwrap();

        // assert thread finished after receiving invalid binary message
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_unsupported_text_message() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx
            .send(Ok(Message::Text("any text message".to_string())))
            .await
            .unwrap();

        // assert thread finished after receiving text message
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_invalid_pong_message() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx
            .send(Ok(Message::Pong(vec![0x00, 0x01])))
            .await
            .unwrap();

        // assert thread finished after receiving invalid pong
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_close_message() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx.send(Ok(Message::Close(None))).await.unwrap();

        // assert thread finished after receiving close message
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn response_read_error() {
        let config = create_test_config();

        let (handle, mut ws_tx, _ws_rx, _notifications_tx) = start_test_connection(config);

        ws_tx
            .send(Err(axum::Error::new("unexpected read error")))
            .await
            .unwrap();

        // assert thread finished after receiving error
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn new_message_sent_to_the_user() {
        let config = create_test_config();

        let (_handle, _ws_tx, mut ws_rx, notifications_tx) = start_test_connection(config);

        let notification = b"this content should be sent to the user".to_vec();
        let message = Arc::new(WebSocketMessage {
            message_id: Uuid::new_v4(),
            payload: notification.clone(),
            delivered_callback: None,
        });

        let _ = notifications_tx.send(message.clone());

        let received_message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap();
        let Message::Binary(received_bytes) = received_message else {
            panic!("invalid message type");
        };

        assert_eq!(received_bytes, notification);
    }

    #[tokio::test]
    async fn new_message_connection_closed() {
        let config = create_test_config();

        let (handle, _ws_tx, ws_rx, notifications_tx) = start_test_connection(config);

        drop(ws_rx);

        let message = Arc::new(WebSocketMessage {
            message_id: Uuid::new_v4(),
            payload: b"ignore me".to_vec(),
            delivered_callback: None,
        });

        let _ = notifications_tx.send(message.clone());

        // thread should finish after send error
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_message_channel_lagged() {
        let config = create_test_config();

        let (handle, _ws_tx, _ws_rx, notifications_tx) = start_test_connection(config);

        let message = Arc::new(WebSocketMessage {
            message_id: Uuid::new_v4(),
            payload: b"ignore".to_vec(),
            delivered_callback: None,
        });

        // send messages until channel is lagged
        let mut is_lagged = false;
        while !is_lagged {
            let send_result = notifications_tx.send(message.clone());
            is_lagged = send_result.is_err();
        }

        // task should finish after channel has lagged
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn new_message_channel_closed() {
        let config = create_test_config();

        let (handle, _ws_tx, _ws_rx, notifications_tx) = start_test_connection(config);

        // drop channel to close it
        drop(notifications_tx);

        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    #[tokio::test]
    async fn queued_message_resend_until_confirmed() {
        let time_begin = OffsetDateTime::now_utc();
        let retry_interval = Duration::from_millis(50);

        let mut config = create_test_config();
        config.retry_interval = retry_interval;
        config.retry_max_count = u8::MAX;

        let (_handle, mut ws_tx, mut ws_rx, notifications_tx) = start_test_connection(config);

        let message_id = Uuid::new_v4();
        let message = Arc::new(WebSocketMessage {
            message_id,
            payload: b"ignore".to_vec(),
            delivered_callback: None,
        });

        let _ = notifications_tx.send(message);

        // read first message
        let message = timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap() // timeout
            .unwrap();
        let Message::Binary(first_message) = message else {
            panic!("invalid message type");
        };

        // keep reading resent message
        for i in 1..10 {
            let message = timeout(Duration::from_secs(1), ws_rx.next())
                .await
                .unwrap() // timeout
                .unwrap();
            let Message::Binary(resent_message) = message else {
                panic!("invalid message type");
            };

            // assert that message_id was not changed
            assert_eq!(resent_message, first_message);

            // assert that message is not sent too often
            let now = OffsetDateTime::now_utc();
            assert!(now >= time_begin + (retry_interval * i));
        }

        let confirmation = input::WebSocketConfirmationProtobuf {
            message_id: message_id.to_string(),
        };
        let confirmation = input::WebSocketConfirmationProtobuf::encode_to_vec(&confirmation);

        // send confirmation to stop resending the message
        ws_tx.send(Ok(Message::Binary(confirmation))).await.unwrap();

        // after sending confirmation, response message should be not resent anymore
        let timeout_result = timeout(Duration::from_millis(300), ws_rx.next()).await;
        assert!(timeout_result.is_err());
    }

    #[tokio::test]
    async fn queued_message_resend_until_limit_reached() {
        let retry_max_count = 4;

        let mut config = create_test_config();
        config.retry_interval = Duration::from_millis(50);
        config.retry_max_count = retry_max_count;

        let (handle, _ws_tx, mut ws_rx, notifications_tx) = start_test_connection(config);

        let message = Arc::new(WebSocketMessage {
            message_id: Uuid::new_v4(),
            payload: b"ignore".to_vec(),
            delivered_callback: None,
        });

        let _ = notifications_tx.send(message);

        // await first message
        timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap()
            .unwrap();

        // await all retries
        for _ in 0..retry_max_count {
            timeout(Duration::from_secs(1), ws_rx.next())
                .await
                .unwrap()
                .unwrap();
        }

        // since message was not confirmed thread should finish
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap(); // thread should never panic
    }

    #[tokio::test]
    async fn queued_message_connection_closed() {
        let mut config = create_test_config();
        config.retry_interval = Duration::from_millis(50);
        config.retry_max_count = u8::MAX;

        let (handle, _ws_tx, mut ws_rx, notifications_tx) = start_test_connection(config);

        // send message that should be resent
        let message = Arc::new(WebSocketMessage {
            message_id: Uuid::new_v4(),
            payload: b"ignore".to_vec(),
            delivered_callback: None,
        });

        let _ = notifications_tx.send(message);

        // await first message
        timeout(Duration::from_secs(1), ws_rx.next())
            .await
            .unwrap()
            .unwrap();

        // drop websocket connection
        drop(ws_rx);

        // when resending message connection should return an error
        // which lead to closing thread
        timeout(Duration::from_secs(1), handle)
            .await
            .unwrap() // timeout
            .unwrap();
    }

    ///
    /// Creates config that won't interfere with tests
    ///
    fn create_test_config() -> WebSocketsServiceConfig {
        WebSocketsServiceConfig {
            ping_interval: Duration::from_secs(1200),
            retry_interval: Duration::from_secs(1200),
            retry_max_count: u8::MAX,
            connection_buffer_size: u8::MAX,
        }
    }

    ///
    /// Starts task with connection.
    ///
    /// ### returns
    /// - task handle
    /// - ws_client_tx - client side send channel
    /// - ws_client_rx - client side read channel
    /// - notifications_tx - channel to pass new messages to the connection
    ///
    fn start_test_connection(
        config: WebSocketsServiceConfig,
    ) -> (
        tokio::task::JoinHandle<()>,
        futures::channel::mpsc::UnboundedSender<Result<Message, axum::Error>>,
        futures::channel::mpsc::UnboundedReceiver<Message>,
        broadcast::Sender<Arc<WebSocketMessage>>,
    ) {
        let (ws_server_tx, ws_client_rx) = futures::channel::mpsc::unbounded();
        let (ws_client_tx, ws_server_rx) = futures::channel::mpsc::unbounded();
        let (messages_tx, messages_rx) = broadcast::channel(4);

        let ws_connection = WebSocketConnection::new(
            Arc::new(config),
            Uuid::new_v4(),
            "0.0.0.0:1234".parse().unwrap(),
            messages_rx,
            ws_server_tx,
            ws_server_rx,
        );

        let handle = tokio::spawn(ws_connection.run());

        (handle, ws_client_tx, ws_client_rx, messages_tx)
    }
}

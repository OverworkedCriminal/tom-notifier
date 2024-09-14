use super::NotificationsDeduplicationServiceConfig;
use bson::oid::ObjectId;
use std::{collections::HashMap, sync::Arc, time::Duration};
use time::OffsetDateTime;
use tokio::{
    sync::{Mutex, Notify},
    time::{interval, Interval, MissedTickBehavior},
};

pub struct NotificationsDeduplicationServiceGarbageCollector {
    last_updates: Arc<Mutex<HashMap<ObjectId, OffsetDateTime>>>,

    interval: Interval,
    notification_lifespan: Duration,
}

impl NotificationsDeduplicationServiceGarbageCollector {
    pub fn new(
        config: NotificationsDeduplicationServiceConfig,
        last_updates: Arc<Mutex<HashMap<ObjectId, OffsetDateTime>>>,
    ) -> Self {
        let mut interval = interval(config.garbage_collector_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        Self {
            last_updates,
            interval,
            notification_lifespan: config.notification_lifespan,
        }
    }

    #[tracing::instrument(name = "Deduplication Garbage Collector", skip_all)]
    pub async fn run(mut self, close_notify: Arc<Notify>) {
        tokio::select! {
            biased;

            // Wait for signal to close
            _ = close_notify.notified() => {},

            // Run infinite loop and remove entries from map periodically
            _ = async { loop {
                self.interval.tick().await;
                let min_timestamp = OffsetDateTime::now_utc() - self.notification_lifespan;

                tracing::debug!("garbage collection started");

                let len_before;
                let len_after;
                {
                    let mut map = self.last_updates.lock().await;
                    len_before = map.len();

                    map.retain(|_, timestamp| *timestamp > min_timestamp);
                    len_after = map.len();

                    if map.len() < map.capacity() / 4 {
                        let new_capacity = map.capacity() / 2;
                        map.shrink_to(new_capacity);
                    }
                }

                let removed_entries = len_before - len_after;
                tracing::debug!(removed_entries, "garbage collection finished");
            }} => {}
        }
    }
}

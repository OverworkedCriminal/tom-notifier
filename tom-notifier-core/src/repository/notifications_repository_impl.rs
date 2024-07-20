use super::{
    dto::{InsertedNotification, Notification, NotificationFindEntity},
    Error, NotificationsRepository,
};
use crate::{dto::input, repository::entity::NotificationInsertEntity};
use axum::async_trait;
use bson::{doc, oid::ObjectId, spec::BinarySubtype, Binary, Bson, DateTime, Document};
use futures_util::TryStreamExt;
use mongodb::{
    error::{ErrorKind, WriteFailure},
    options::IndexOptions,
    Collection, Database, IndexModel,
};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

const NOTIFICATIONS: &str = "notifications";
const INDEX_NAME_UNIQUE_PRODUCER_NOTIFICATION: &str =
    "unique_index_producer_id_producer_notification_id";
const INDEX_NAME_CONFIRMATION_USER_ID: &str = "index_confirmation_user_id";

pub struct NotificationsRepositoryImpl {
    database: Database,
}

impl NotificationsRepositoryImpl {
    pub async fn new(database: Database) -> Result<Self, mongodb::error::Error> {
        database.create_collection(NOTIFICATIONS).await?;

        let collection = database.collection(NOTIFICATIONS);
        let index_names = collection.list_index_names().await?;

        if !index_names.contains(&INDEX_NAME_UNIQUE_PRODUCER_NOTIFICATION.to_string()) {
            Self::create_unique_producer_notification_index(&collection).await?;
            tracing::debug!(
                "created index {NOTIFICATIONS}.{INDEX_NAME_UNIQUE_PRODUCER_NOTIFICATION}"
            );
        }
        if !index_names.contains(&INDEX_NAME_CONFIRMATION_USER_ID.to_string()) {
            Self::create_confirmations_user_id_index(&collection).await?;
            tracing::debug!("created index {NOTIFICATIONS}.{INDEX_NAME_CONFIRMATION_USER_ID}");
        }

        Ok(Self { database })
    }

    async fn create_unique_producer_notification_index(
        collection: &Collection<Document>,
    ) -> Result<(), mongodb::error::Error> {
        let index = IndexModel::builder()
            .keys(doc! {
                "producer_id": 1,
                "producer_notification_id": 1,
            })
            .options(
                IndexOptions::builder()
                    .name(INDEX_NAME_UNIQUE_PRODUCER_NOTIFICATION.to_string())
                    .unique(true)
                    .build(),
            )
            .build();

        collection.create_index(index).await?;

        Ok(())
    }

    async fn create_confirmations_user_id_index(
        collection: &Collection<Document>,
    ) -> Result<(), mongodb::error::Error> {
        let index = IndexModel::builder()
            .keys(doc! {
                "confirmations.user_id": 1,
            })
            .options(
                IndexOptions::builder()
                    .name(INDEX_NAME_CONFIRMATION_USER_ID.to_string())
                    .build(),
            )
            .build();

        collection.create_index(index).await?;

        Ok(())
    }
}

#[async_trait]
impl NotificationsRepository for NotificationsRepositoryImpl {
    async fn insert(
        &self,
        user_ids: Vec<Uuid>,
        created_at: OffsetDateTime,
        invalidate_at: Option<OffsetDateTime>,
        producer_id: Uuid,
        producer_notification_id: i64,
        content_type: String,
        content: Vec<u8>,
    ) -> Result<InsertedNotification, Error> {
        let insert_entity = NotificationInsertEntity {
            created_at: DateTime::from(created_at),
            invalidate_at: invalidate_at.map(DateTime::from),
            user_ids: user_ids
                .iter()
                .map(|user_id| bson::Uuid::from(*user_id))
                .collect(),
            producer_id: producer_id.into(),
            producer_notification_id,
            content_type,
            content: Binary {
                subtype: BinarySubtype::Generic,
                bytes: content,
            },
            confirmations: [],
        };

        let insert_result = self
            .database
            .collection::<NotificationInsertEntity>(NOTIFICATIONS)
            .insert_one(&insert_entity)
            .await
            .map_err(|err| {
                let ErrorKind::Write(ref write_failure) = *err.kind else {
                    return Error::Mongo(err);
                };

                let WriteFailure::WriteError(write_error) = write_failure else {
                    return Error::Mongo(err);
                };

                const DUPLICATE_KEY_CODE: i32 = 11000;
                match write_error.code == DUPLICATE_KEY_CODE {
                    true => Error::InsertUniqueViolation,
                    false => Error::Mongo(err),
                }
            })?;

        let Bson::ObjectId(id) = insert_result.inserted_id else {
            tracing::error!("invalid type of inserted '_id'");
            return Err(Error::Mongo(
                ErrorKind::Custom(Arc::new("invalid type of inserted '_id'")).into(),
            ));
        };

        Ok(InsertedNotification {
            id,
            created_at,
            invalidate_at,
            user_ids,
            producer_id,
            producer_notification_id,
            content_type: insert_entity.content_type,
            content: insert_entity.content.bytes,
        })
    }

    async fn update_invalidate_at(
        &self,
        id: ObjectId,
        producer_id: Uuid,
        invalidate_at: Option<OffsetDateTime>,
    ) -> Result<(), Error> {
        let producer_id = bson::Uuid::from(producer_id);
        let invalidate_at = invalidate_at.map(DateTime::from);

        let update_result = self
            .database
            .collection::<Document>(NOTIFICATIONS)
            .update_one(
                doc! {
                    "_id": id,
                    "producer_id": producer_id,
                },
                doc! {
                    "$set": {
                        "invalidate_at": invalidate_at,
                    }
                },
            )
            .await?;

        match update_result.matched_count == 1 {
            true => Ok(()),
            false => Err(Error::NoDocumentUpdated),
        }
    }

    async fn insert_confirmation(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error> {
        let user_id = bson::Uuid::from(user_id);
        let now = DateTime::from(OffsetDateTime::now_utc());

        let update_result = self
            .database
            .collection::<Document>(NOTIFICATIONS)
            .update_one(
                doc! {
                    "_id": id,
                    "$and": [
                        {
                            "$or": [
                                { "user_ids": user_id },
                                { "user_ids": { "$size": 0 } },
                            ]
                        },
                        {
                            "$or": [
                                { "invalidate_at": None as Option<DateTime> },
                                { "invalidate_at": { "$gt": now } },
                            ]
                        },
                    ],
                    "confirmations": {
                        "$not": {
                            "$elemMatch": {
                                "user_id": user_id,
                            }
                        }
                    }
                },
                doc! {
                    "$push": {
                        "confirmations": {
                            "user_id": user_id,
                            "notification_delivered_at": now,
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    }
                },
            )
            .await?;

        match update_result.modified_count > 0 {
            true => Ok(()),
            false => Err(Error::NoDocumentUpdated),
        }
    }

    async fn insert_many_confirmations(
        &self,
        ids: &[ObjectId],
        user_id: Uuid,
    ) -> Result<(), Error> {
        let user_id = bson::Uuid::from(user_id);
        let now = DateTime::from(OffsetDateTime::now_utc());

        self.database
            .collection::<Document>(NOTIFICATIONS)
            .update_many(
                doc! {
                    "_id": { "$in": ids },
                    "$and": [
                        {
                            "$or": [
                                { "user_ids": user_id },
                                { "user_ids": { "$size": 0 } },
                            ]
                        },
                        {
                            "$or": [
                                { "invalidate_at": None as Option<DateTime> },
                                { "invalidate_at": { "$gt": now } },
                            ]
                        },
                    ],
                    "confirmations": {
                        "$not": {
                            "$elemMatch": {
                                "user_id": user_id,
                            }
                        }
                    }
                },
                doc! {
                    "$push": {
                        "confirmations": {
                            "user_id": user_id,
                            "notification_delivered_at": now,
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    }
                },
            )
            .await?;

        Ok(())
    }

    async fn update_confirmation_seen(
        &self,
        id: ObjectId,
        user_id: Uuid,
        seen: bool,
    ) -> Result<(), Error> {
        let user_id = bson::Uuid::from(user_id);

        let update_result = self
            .database
            .collection::<Document>(NOTIFICATIONS)
            .update_one(
                doc! {
                    "_id": id,
                    "confirmations": {
                        "$elemMatch": {
                            "user_id": user_id,
                            "notification_deleted": false,
                        }
                    }
                },
                doc! {
                    "$set": {
                        "confirmations.$.notification_seen": seen,
                    }
                },
            )
            .await?;

        // matched_count instead of modified_count becaouse replacing
        // false with false doesn't count as modification
        match update_result.matched_count == 1 {
            true => Ok(()),
            false => Err(Error::NoDocumentUpdated),
        }
    }

    async fn delete(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error> {
        let user_id = bson::Uuid::from(user_id);

        let update_result = self
            .database
            .collection::<Document>(NOTIFICATIONS)
            .update_one(
                doc! {
                    "_id": id,
                    "confirmations": {
                        "$elemMatch": {
                            "user_id": user_id,
                        }
                    }
                },
                doc! {
                    "$set": {
                        "confirmations.$.notification_deleted": true,
                    }
                },
            )
            .await?;

        match update_result.modified_count == 1 {
            true => Ok(()),
            false => Err(Error::NoDocumentUpdated),
        }
    }

    async fn find_delivered(
        &self,
        id: ObjectId,
        user_id: Uuid,
    ) -> Result<Option<Notification>, Error> {
        let user_id = bson::Uuid::from(user_id);

        let notification_entity = self
            .database
            .collection::<NotificationFindEntity>(NOTIFICATIONS)
            .find_one(doc! {
                "_id": id,
                "confirmations": {
                    "$elemMatch": {
                        "user_id": user_id,
                        "notification_deleted": false,
                    }
                }
            })
            .projection(doc! {
                "_id": 1,
                "created_at": 1,
                "producer_id": 1,
                "content_type": 1,
                "content": 1,
                "confirmations.$": 1,
            })
            .await?;

        let notification = notification_entity.map(Notification::from);

        Ok(notification)
    }

    async fn find_many_delivered(
        &self,
        user_id: Uuid,
        pagination: input::Pagination,
        input::NotificationFilters { seen }: input::NotificationFilters,
    ) -> Result<Vec<Notification>, Error> {
        let user_id = bson::Uuid::from(user_id);
        let mut confirmation_filter = doc! {
            "user_id": user_id,
            "notification_deleted": false,
        };
        if let Some(seen) = seen {
            confirmation_filter.insert("notification_seen", seen);
        }

        let cursor = self
            .database
            .collection::<NotificationFindEntity>(NOTIFICATIONS)
            .find(doc! {
                "confirmations": {
                    "$elemMatch": confirmation_filter,
                }
            })
            .projection(doc! {
                "_id": 1,
                "created_at": 1,
                "producer_id": 1,
                "content_type": 1,
                "content": 1,
                "confirmations.$": 1,
            })
            .sort(doc! {
                "created_at": -1
            })
            .skip((pagination.page_size * pagination.page_idx) as u64)
            .limit(pagination.page_size as i64)
            .await?;

        let notifications = cursor.map_ok(Notification::from).try_collect().await?;

        Ok(notifications)
    }

    async fn find_many_undelivered(&self, user_id: Uuid) -> Result<Vec<Notification>, Error> {
        let user_id = bson::Uuid::from(user_id);
        let now = DateTime::from(OffsetDateTime::now_utc());

        let notifications = self
            .database
            .collection::<NotificationFindEntity>(NOTIFICATIONS)
            .find(doc! {
                "$and": [
                    {
                        "$or": [
                            { "user_ids": user_id },
                            { "user_ids": { "$size": 0 } },
                        ]
                    },
                    {
                        "$or": [
                            { "invalidate_at": { "$gt": now } },
                            { "invalidate_at": None as Option<DateTime> },
                        ],
                    }
                ],
                "confirmations": {
                    "$not": {
                        "$elemMatch": {
                            "user_id": user_id,
                        }
                    }
                },
            })
            .projection(doc! {
                "_id": 1,
                "created_at": 1,
                "producer_id": 1,
                "content_type": 1,
                "content": 1,
            })
            .sort(doc! { "created_at": 1 })
            .await?
            .map_ok(Notification::from)
            .try_collect()
            .await?;

        Ok(notifications)
    }
}

///
/// Tests require env variables to be set and database to be running
///
#[cfg(test)]
mod test {
    use std::time::Duration;

    use super::*;
    use crate::application::ApplicationEnv;
    use anyhow::{anyhow, Context};
    use mongodb::{options::ClientOptions, Client};
    use time::macros::datetime;

    async fn create_test_database() -> anyhow::Result<Database> {
        let env = ApplicationEnv::parse().context("failed to parse env variables")?;
        let db_name = format!("test_{}_{}", env.db_name, Uuid::new_v4());

        let db_client_options = ClientOptions::parse(&env.db_connection_string).await?;
        let db_client = Client::with_options(db_client_options)?;
        let db = db_client.database(&db_name);

        Ok(db)
    }

    async fn destroy_test_database(database: Database) {
        let _ = database.drop().await;
        database.client().clone().shutdown().await;
    }

    #[tokio::test]
    async fn insert_correct_created_at() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let mut inserted_created_at = OffsetDateTime::now_utc();

        let mut notification = repository
            .insert(
                vec![],
                inserted_created_at,
                None,
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let datetime = document.get_datetime("created_at")?;
        let created_at = OffsetDateTime::from(*datetime);

        // Mongo keeps datetime in milliseconds so it's
        // necessary to set micro and nanoseconds to 0
        inserted_created_at =
            inserted_created_at.replace_millisecond(inserted_created_at.millisecond())?;
        notification.created_at = notification
            .created_at
            .replace_millisecond(notification.created_at.millisecond())?;

        assert_eq!(created_at, inserted_created_at);
        assert_eq!(created_at, notification.created_at);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_invalidate_at() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_invalidate_at = datetime!(2024-01-21 19:06:25 UTC);

        let notification = repository
            .insert(
                vec![],
                OffsetDateTime::now_utc(),
                Some(inserted_invalidate_at),
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let datetime = document.get_datetime("invalidate_at")?;
        let invalidate_at = OffsetDateTime::from(*datetime);

        assert_eq!(invalidate_at, inserted_invalidate_at);
        assert_eq!(invalidate_at, notification.invalidate_at.unwrap());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_invalidate_at_none() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_invalidate_at = None;

        let notification = repository
            .insert(
                vec![],
                OffsetDateTime::now_utc(),
                inserted_invalidate_at,
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let invalidate_at = document.get("invalidate_at").unwrap().as_null();

        assert!(matches!(invalidate_at, Some(())));
        assert!(notification.invalidate_at.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_user_ids_unicast() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_user_id = Uuid::from_u128(48190238120);

        let notification = repository
            .insert(
                vec![inserted_user_id],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let user_ids = document.get_array("user_ids")?;
        assert_eq!(user_ids.len(), 1);

        let user_id = user_ids.first().unwrap();
        let Bson::Binary(binary) = user_id else {
            panic!("invalid user_id type");
        };
        let user_id = binary.to_uuid()?;
        let user_id = Uuid::from(user_id);

        assert_eq!(user_id, inserted_user_id);
        assert_eq!(user_id, *notification.user_ids.first().unwrap());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_user_ids_multicast() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_user_ids = vec![Uuid::from_u128(48190238120), Uuid::from_u128(11212039809)];

        let notification = repository
            .insert(
                inserted_user_ids.clone(),
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let user_ids = document
            .get_array("user_ids")?
            .into_iter()
            .filter_map(|bson| {
                let Bson::Binary(binary) = bson else {
                    return None;
                };
                let Ok(user_id) = binary.to_uuid() else {
                    return None;
                };
                Some(user_id.into())
            })
            .collect::<Vec<Uuid>>();
        assert_eq!(user_ids.len(), inserted_user_ids.len());
        assert_eq!(user_ids.len(), notification.user_ids.len());

        let database_ok = inserted_user_ids
            .iter()
            .all(|inserted_user_id| user_ids.contains(inserted_user_id));
        let notification_ok = inserted_user_ids
            .iter()
            .all(|inserted_user_id| notification.user_ids.contains(inserted_user_id));

        assert!(database_ok);
        assert!(notification_ok);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_user_ids_empty() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let notification = repository
            .insert(
                vec![],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(3214098123091),
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let user_ids = document.get_array("user_ids")?;

        assert!(user_ids.is_empty());
        assert!(notification.user_ids.is_empty());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_producer_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_producer_id = Uuid::from_u128(4812038102);

        let notification = repository
            .insert(
                vec![Uuid::from_u128(8129381)],
                OffsetDateTime::now_utc(),
                None,
                inserted_producer_id,
                1,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let producer_id = document.get("producer_id").unwrap();
        let Bson::Binary(binary) = producer_id else {
            panic!("invalid type");
        };
        let producer_id = binary.to_uuid()?;
        let producer_id = Uuid::from(producer_id);

        assert_eq!(producer_id, inserted_producer_id);
        assert_eq!(producer_id, notification.producer_id);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_producer_notification_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_producer_notification_id = 721;

        let notification = repository
            .insert(
                vec![],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(3214098123091),
                inserted_producer_notification_id,
                "utf-8".to_string(),
                b"not important content".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let producer_notification_id = document.get_i64("producer_notification_id")?;

        assert_eq!(producer_notification_id, inserted_producer_notification_id);
        assert_eq!(
            producer_notification_id,
            notification.producer_notification_id
        );

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_content_type() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_content_type = "json".to_string();

        let notification = repository
            .insert(
                vec![Uuid::from_u128(8129381)],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(1231203),
                1,
                inserted_content_type.clone(),
                br#"{"value":null}"#.to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let content_type = document.get_str("content_type")?;

        assert_eq!(content_type, inserted_content_type);
        assert_eq!(content_type, notification.content_type);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_content() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let inserted_content = br#"{"value": null}"#.to_vec();

        let notification = repository
            .insert(
                vec![Uuid::from_u128(8129381)],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(1231203),
                1,
                "json".to_string(),
                inserted_content.clone(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let content = document.get_binary_generic("content")?;

        assert_eq!(*content, inserted_content);
        assert_eq!(*content, notification.content);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_correct_confirmations() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let notification = repository
            .insert(
                vec![Uuid::from_u128(8129381)],
                OffsetDateTime::now_utc(),
                None,
                Uuid::from_u128(1231203),
                1,
                "utf-8".to_string(),
                b"data".to_vec(),
            )
            .await?;

        let document = collection
            .find_one(doc! { "_id": notification.id })
            .await?
            .unwrap();
        let confirmations = document.get_array("confirmations")?;

        assert!(confirmations.is_empty());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_duplicated_producer_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let producer_id = Uuid::from_u128(7498127391);
        let producer_notification_id = 672;

        // fixture
        collection
            .insert_many([doc! {
                "producer_id": bson::Uuid::from(producer_id),
                "producer_notification_id": producer_notification_id,
            }])
            .await?;

        let insert_result = repository
            .insert(
                vec![Uuid::from_u128(8129381)],
                OffsetDateTime::now_utc(),
                None,
                producer_id,
                producer_notification_id,
                "utf-8".to_string(),
                b"data".to_vec(),
            )
            .await;

        assert!(matches!(insert_result, Err(Error::InsertUniqueViolation)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_invalidate_at_value_updated() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let producer_id = Uuid::from_u128(5791022193);

        collection
            .insert_many([doc! {
                "_id": id,
                "producer_id": bson::Uuid::from(producer_id),
                "invalidate_at": None as Option<DateTime>,
            }])
            .await?;

        let mut inserted_invalidate_at = OffsetDateTime::now_utc() + Duration::from_secs(300);
        repository
            .update_invalidate_at(id, producer_id, Some(inserted_invalidate_at))
            .await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let invalidate_at = document.get_datetime("invalidate_at")?;
        let invalidate_at = OffsetDateTime::from(*invalidate_at);

        inserted_invalidate_at = inserted_invalidate_at
            .replace_millisecond(inserted_invalidate_at.millisecond())
            .unwrap();

        assert_eq!(invalidate_at, inserted_invalidate_at);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_invalidate_at_the_same_value() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let producer_id = Uuid::from_u128(5791022193);
        let invalidate_at = OffsetDateTime::now_utc();

        collection
            .insert_many([doc! {
                "_id": id,
                "producer_id": bson::Uuid::from(producer_id),
                "invalidate_at": DateTime::from(invalidate_at),
            }])
            .await?;

        let update_result = repository
            .update_invalidate_at(id, producer_id, Some(invalidate_at))
            .await;

        assert!(update_result.is_ok());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_invalidate_at_id_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let other_id = ObjectId::new();
        let producer_id = Uuid::from_u128(1);

        collection
            .insert_many([doc! {
                "_id": other_id,
                "producer_id": bson::Uuid::from(producer_id),
                "invalidate_at": None as Option<DateTime>,
            }])
            .await?;

        let update_result = repository
            .update_invalidate_at(id, producer_id, Some(OffsetDateTime::now_utc()))
            .await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_invalidate_at_wrong_producer() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let producer_id = Uuid::from_u128(1);
        let other_producer_id = Uuid::from_u128(5871923123);

        collection
            .insert_many([doc! {
                "_id": id,
                "producer_id": bson::Uuid::from(other_producer_id),
                "invalidate_at": None as Option<DateTime>,
            }])
            .await?;

        let update_result = repository
            .update_invalidate_at(id, producer_id, Some(OffsetDateTime::now_utc()))
            .await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_correct_user_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let inserted_user_id = Uuid::from_u128(48120938012);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(inserted_user_id)],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": [],
            }])
            .await?;

        repository.insert_confirmation(id, inserted_user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };

        let user_id = confirmation.get("user_id").unwrap();
        let Bson::Binary(binary) = user_id else {
            panic!("Invalid user_id type");
        };
        let user_id = binary.to_uuid()?.to_uuid_1();

        assert_eq!(user_id, inserted_user_id);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_correct_notification_delivered_at() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": [],
            }])
            .await?;

        let beg = OffsetDateTime::now_utc();
        repository.insert_confirmation(id, user_id).await?;
        let end = OffsetDateTime::now_utc();

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };
        let notification_delivered_at = confirmation.get_datetime("notification_delivered_at")?;
        let notification_delivered_at = OffsetDateTime::from(*notification_delivered_at);

        // adjust precision to database's precision
        let beg = beg.replace_millisecond(beg.millisecond()).unwrap();
        let end = end.replace_millisecond(end.millisecond()).unwrap();

        assert!(beg <= notification_delivered_at);
        assert!(notification_delivered_at <= end);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_correct_notification_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": [],
            }])
            .await?;

        repository.insert_confirmation(id, user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };
        let deleted = confirmation.get_bool("notification_deleted")?;

        assert_eq!(deleted, false);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_correct_notification_seen() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": [],
            }])
            .await?;

        repository.insert_confirmation(id, user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };
        let seen = confirmation.get_bool("notification_seen")?;

        assert_eq!(seen, false);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_already_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "invalidate_at": None as Option<DateTime>,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 12:34:31 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ],
                }],
            )
            .await?;

        let result = repository.insert_confirmation(id, user_id).await;

        assert!(matches!(result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_other_user_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_id = Uuid::from_u128(41203810);
        let other_user_id = Uuid::from_u128(809123123);
        assert_ne!(user_id, other_user_id);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(other_user_id)],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": [],
            }])
            .await?;

        let result = repository.insert_confirmation(id, user_id).await;

        assert!(matches!(result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_multicast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let id = ObjectId::new();

        let user_1_id = Uuid::from_u128(1);
        let user_2_id = Uuid::from_u128(2);
        let other_user_id = Uuid::from_u128(48210380128310);

        collection
            .insert_many([doc! {
                    "_id": id,
                    "user_ids": [
                        bson::Uuid::from(user_1_id),
                        bson::Uuid::from(user_2_id),
                    ],
                    "invalidate_at": None as Option<DateTime>,
                    "confirmations": [],
            }])
            .await?;

        repository.insert_confirmation(id, user_1_id).await?;
        repository.insert_confirmation(id, user_2_id).await?;

        let other_user_insert_result = repository.insert_confirmation(id, other_user_id).await;

        assert!(matches!(
            other_user_insert_result,
            Err(Error::NoDocumentUpdated)
        ));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_broadcast_notification_correct_user_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(841203981);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "invalidate_at": None as Option<DateTime>,
                "confirmations": []
            }])
            .await?;

        repository.insert_confirmation(id, inserted_user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };

        let user_id = confirmation.get("user_id").unwrap();
        let Bson::Binary(binary) = user_id else {
            panic!("Invalid user_id type");
        };
        let user_id = binary.to_uuid()?.to_uuid_1();

        assert_eq!(user_id, inserted_user_id);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_broadcast_notification_already_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 12:34:31 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ],
                }],
            ).await?;

        let result = repository.insert_confirmation(id, user_id).await;

        assert!(matches!(result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_broadcast_notification_other_users_confirmations_exist(
    ) -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(4);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(2)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 13:24:31 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(1)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 13:24:35 UTC)),
                            "notification_seen": false,
                            "notification_deleted": true,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(5)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 13:24:41 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(3)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-22 13:25:11 UTC)),
                            "notification_seen": true,
                            "notification_deleted": true,
                        }
                    ]
                }]
            ).await?;

        repository.insert_confirmation(id, inserted_user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;

        let contains_new_confirmation = confirmations
            .into_iter()
            .filter_map(|confirmation| match confirmation {
                Bson::Document(confirmation) => Some(confirmation),
                _ => None,
            })
            .any(|confirmation| {
                let Some(user_id) = confirmation.get("user_id") else {
                    return false;
                };
                let Bson::Binary(binary) = user_id else {
                    return false;
                };
                let Ok(user_id) = binary.to_uuid() else {
                    return false;
                };
                let user_id = Uuid::from(user_id);

                user_id == inserted_user_id
            });

        assert_eq!(contains_new_confirmation, true);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_invalidate_at_passed() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(41203810);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "invalidate_at": DateTime::from(datetime!(2024-01-22 13:49:00 UTC)),
                "confirmations": [],
            }])
            .await?;

        let result = repository.insert_confirmation(id, user_id).await;

        assert!(matches!(result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_confirmation_invalidate_at_not_passed() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(41203810);
        let future_datetime = OffsetDateTime::now_utc() + Duration::from_secs(30 * 60);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "invalidate_at": DateTime::from(future_datetime),
                "confirmations": [],
            }])
            .await?;

        repository.insert_confirmation(id, user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;

        assert_eq!(confirmations.is_empty(), false);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_correct_user_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;
            let Bson::Document(confirmation) = confirmations.first().unwrap() else {
                panic!("confirmations is not a document");
            };

            let user_id = confirmation.get("user_id").unwrap();
            let Bson::Binary(binary) = user_id else {
                panic!("Invalid user_id type");
            };
            let user_id = binary.to_uuid()?.to_uuid_1();

            assert_eq!(user_id, inserted_user_id);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_correct_notification_delivered_at() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        let mut beg = OffsetDateTime::now_utc();
        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;
        let mut end = OffsetDateTime::now_utc();

        // adjust to database's precision
        beg = beg.replace_millisecond(beg.millisecond()).unwrap();
        end = end.replace_millisecond(end.millisecond()).unwrap();

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;
            let Bson::Document(confirmation) = confirmations.first().unwrap() else {
                panic!("confirmations is not a document");
            };
            let notification_delivered_at =
                confirmation.get_datetime("notification_delivered_at")?;
            let notification_delivered_at = OffsetDateTime::from(*notification_delivered_at);

            assert!(beg <= notification_delivered_at);
            assert!(notification_delivered_at <= end);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_correct_notification_seen() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;
            let Bson::Document(confirmation) = confirmations.first().unwrap() else {
                panic!("confirmations is not a document");
            };
            let notification_seen = confirmation.get_bool("notification_seen")?;

            assert_eq!(notification_seen, false);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_correct_notification_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": None as Option<DateTime>,
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;
            let Bson::Document(confirmation) = confirmations.first().unwrap() else {
                panic!("confirmations is not a document");
            };
            let notification_deleted = confirmation.get_bool("notification_deleted")?;

            assert_eq!(notification_deleted, false);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_invalidate_at_passed() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": OffsetDateTime::now_utc() - Duration::from_secs(300),
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": OffsetDateTime::now_utc() - Duration::from_secs(300),
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": OffsetDateTime::now_utc() - Duration::from_secs(300),
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;

            assert!(confirmations.is_empty());
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmations_invalidate_at_not_passed() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 1,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 2,
                    "confirmations": [],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 3,
                    "confirmations": [],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;

            assert_eq!(confirmations.is_empty(), false);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn insert_many_confirmatons_skip_notifications_with_confirmations() -> anyhow::Result<()>
    {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id_1 = ObjectId::new();
        let id_2 = ObjectId::new();
        let id_3 = ObjectId::new();
        let inserted_user_id = Uuid::from_u128(75917293871);
        let other_user_id = Uuid::from_u128(12412908123);

        collection
            .insert_many([
                doc! {
                    "_id": id_1,
                    "user_ids": [],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 1,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(inserted_user_id),
                        }
                    ],
                },
                doc! {
                    "_id": id_2,
                    "user_ids": [bson::Uuid::from(inserted_user_id)],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 2,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(inserted_user_id),
                        }
                    ],
                },
                doc! {
                    "_id": id_3,
                    "user_ids": [
                        bson::Uuid::from(inserted_user_id),
                        bson::Uuid::from(other_user_id),
                    ],
                    "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                    "producer_notification_id": 3,
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(inserted_user_id),
                        }
                    ],
                },
            ])
            .await?;

        let ids = [id_1, id_2, id_3];

        repository
            .insert_many_confirmations(&ids, inserted_user_id)
            .await?;

        for id in ids {
            let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
            let confirmations = document.get_array("confirmations")?;

            assert_eq!(confirmations.len(), 1);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_value_changed() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);
        let seen = true;
        let updated_seen = !seen;

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_seen": seen,
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository
            .update_confirmation_seen(id, user_id, updated_seen)
            .await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };

        let seen = confirmation.get_bool("notification_seen")?;

        assert_eq!(seen, updated_seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_the_same_value() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);
        let seen = true;
        let updated_seen = seen;

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_seen": seen,
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository
            .update_confirmation_seen(id, user_id, updated_seen)
            .await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };

        let seen = confirmation.get_bool("notification_seen")?;

        assert_eq!(seen, updated_seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_notification_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);

        let update_result = repository.update_confirmation_seen(id, user_id, true).await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_other_user_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);
        let other_user_id = Uuid::from_u128(41820983102);
        assert_ne!(user_id, other_user_id);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(other_user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(other_user_id),
                        "notification_seen": false,
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        let update_result = repository.update_confirmation_seen(id, user_id, true).await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_confirmation_does_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": []
            }])
            .await?;

        let update_result = repository.update_confirmation_seen(id, user_id, true).await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_notification_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_seen": false,
                        "notification_deleted": true,
                    }
                ]
            }])
            .await?;

        let update_result = repository.update_confirmation_seen(id, user_id, true).await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_multicast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_1_id = Uuid::from_u128(1);
        let user_2_id = Uuid::from_u128(2);
        let other_user_id = Uuid::from_u128(5810293810238);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [
                    bson::Uuid::from(user_1_id),
                    bson::Uuid::from(user_2_id),
                ],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_1_id),
                        "notification_seen": false,
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(user_2_id),
                        "notification_seen": false,
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository
            .update_confirmation_seen(id, user_1_id, true)
            .await?;
        repository
            .update_confirmation_seen(id, user_2_id, true)
            .await?;

        let update_result = repository
            .update_confirmation_seen(id, other_user_id, true)
            .await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_broadcast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(3819028301);
        let seen = true;
        let updated_seen = !seen;

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_seen": seen,
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository
            .update_confirmation_seen(id, user_id, updated_seen)
            .await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;

        let confirmation = confirmations
            .into_iter()
            .filter_map(|confirmation| {
                let Bson::Document(confirmation) = confirmation else {
                    return None;
                };
                let Some(confirmation_user_id) = confirmation.get("user_id") else {
                    return None;
                };
                let Bson::Binary(binary) = confirmation_user_id else {
                    return None;
                };
                let Ok(confirmation_user_id) = binary.to_uuid() else {
                    return None;
                };
                let confirmation_user_id = Uuid::from(confirmation_user_id);
                match confirmation_user_id == user_id {
                    true => Some(confirmation),
                    false => None,
                }
            })
            .next()
            .unwrap();

        let seen = confirmation.get_bool("notification_seen")?;

        assert_eq!(seen, updated_seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_broadcast_notification_confirmation_not_exist(
    ) -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(3120312093)),
                        "notification_seen": false,
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(58102380192)),
                        "notification_seen": false,
                        "notification_deleted": true,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(48120312092)),
                        "notification_seen": true,
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(81023812012)),
                        "notification_seen": true,
                        "notification_deleted": true,
                    },
                ]
            }])
            .await?;

        let update_result = repository.update_confirmation_seen(id, user_id, true).await;

        assert!(matches!(update_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn update_confirmation_seen_broadcast_notification_has_multiple_confirmations(
    ) -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);
        let seen = true;
        let updated_seen = !seen;

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(3120312093)),
                        "notification_seen": false,
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(58102380192)),
                        "notification_seen": false,
                        "notification_deleted": true,
                    },
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_seen": true,
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(81023812012)),
                        "notification_seen": true,
                        "notification_deleted": true,
                    },
                ]
            }])
            .await?;

        repository
            .update_confirmation_seen(id, user_id, updated_seen)
            .await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;

        let confirmation = confirmations
            .into_iter()
            .filter_map(|confirmation| {
                let Bson::Document(confirmation) = confirmation else {
                    return None;
                };
                let Some(confirmation_user_id) = confirmation.get("user_id") else {
                    return None;
                };
                let Bson::Binary(binary) = confirmation_user_id else {
                    return None;
                };
                let Ok(confirmation_user_id) = binary.to_uuid() else {
                    return None;
                };
                let confirmation_user_id = Uuid::from(confirmation_user_id);
                match confirmation_user_id == user_id {
                    true => Some(confirmation),
                    false => None,
                }
            })
            .next()
            .unwrap();

        let seen = confirmation.get_bool("notification_seen")?;

        assert_eq!(seen, updated_seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_notification_deleted_set_to_true() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(48120381029);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository.delete(id, user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document.get_array("confirmations")?;
        let Bson::Document(confirmation) = confirmations.first().unwrap() else {
            panic!("confirmations is not a document");
        };

        let deleted = confirmation.get_bool("notification_deleted")?;

        assert_eq!(deleted, true);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_confirmation_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(48120381029);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": []
            }])
            .await?;

        let delete_result = repository.delete(id, user_id).await;

        assert!(matches!(delete_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_already_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(48120381029);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_deleted": true,
                    }
                ]
            }])
            .await?;

        let delete_result = repository.delete(id, user_id).await;

        assert!(matches!(delete_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_notification_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(48120381029);

        let delete_result = repository.delete(id, user_id).await;

        assert!(matches!(delete_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_other_user_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(48120381029);
        let other_user_id = Uuid::from_u128(4820193801283);
        assert_ne!(user_id, other_user_id);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [bson::Uuid::from(other_user_id)],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(other_user_id),
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        let delete_result = repository.delete(id, user_id).await;

        assert!(matches!(delete_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_multicast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_1_id = Uuid::from_u128(1);
        let user_2_id = Uuid::from_u128(2);
        let other_user_id = Uuid::from_u128(50238210);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [
                    bson::Uuid::from(user_1_id),
                    bson::Uuid::from(user_2_id),
                ],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(user_1_id),
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(user_2_id),
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        let other_user_delete_result = repository.delete(id, other_user_id).await;
        assert!(matches!(
            other_user_delete_result,
            Err(Error::NoDocumentUpdated)
        ));

        repository.delete(id, user_1_id).await?;
        repository.delete(id, user_2_id).await?;

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_broadcast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(48012983019283)),
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(user_id),
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(809841812308)),
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        repository.delete(id, user_id).await?;

        let document = collection.find_one(doc! { "_id": id }).await?.unwrap();
        let confirmations = document
            .get_array("confirmations")?
            .into_iter()
            .filter_map(|confirmation| match confirmation {
                Bson::Document(confirmation) => {
                    let Some(confirmation_user_id) = confirmation.get("user_id") else {
                        return None;
                    };
                    let Bson::Binary(binary) = confirmation_user_id else {
                        return None;
                    };
                    let Ok(confirmation_user_id) = binary.to_uuid() else {
                        return None;
                    };
                    let confirmation_user_id = Uuid::from(confirmation_user_id);
                    let Ok(deleted) = confirmation.get_bool("notification_deleted") else {
                        return None;
                    };
                    Some((confirmation_user_id, deleted))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(confirmations.len(), 3);

        let deleted = confirmations
            .iter()
            .filter_map(|(confirmation_user_id, deleted)| {
                match Uuid::from(*confirmation_user_id) == user_id {
                    true => Some(deleted),
                    false => None,
                }
            })
            .next()
            .expect("Could not find user_id in confirmations");
        assert_eq!(*deleted, true);

        let rest_unchanged = confirmations
            .iter()
            .filter_map(|(confirmation_user_id, deleted)| {
                match Uuid::from(*confirmation_user_id) != user_id {
                    true => Some(deleted),
                    false => None,
                }
            })
            .all(|deleted| *deleted == false);

        assert_eq!(rest_unchanged, true);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn delete_broadcast_notification_confirmation_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many([doc! {
                "_id": id,
                "user_ids": [],
                "confirmations": [
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(48012983019283)),
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(3841200192)),
                        "notification_deleted": false,
                    },
                    {
                        "user_id": bson::Uuid::from(Uuid::from_u128(809841812308)),
                        "notification_deleted": false,
                    }
                ]
            }])
            .await?;

        let delete_result = repository.delete(id, user_id).await;

        assert!(matches!(delete_result, Err(Error::NoDocumentUpdated)));

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_correct_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let other_id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        assert_ne!(id, other_id);

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 11:11:11 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "_id": other_id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:42:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 11:43:11 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?.unwrap();

        assert_eq!(notification.id, id);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_correct_created_at() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        let created_at = datetime!(2024-02-10 08:46:12.123 UTC);

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(created_at),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?.unwrap();

        assert_eq!(notification.created_at, created_at);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_correct_seen() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        let seen = true;

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-02-10 08:46:12.123 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": seen,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?.unwrap();

        assert_eq!(notification.seen, seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_mutlicast_notification_correct_seen() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_1_id = Uuid::from_u128(85190238019);
        let user_2_id = Uuid::from_u128(50128301822);
        let user_1_seen = true;
        let user_2_seen = false;

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-02-10 08:46:12.123 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(user_1_id),
                        bson::Uuid::from(user_2_id),
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_1_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": user_1_seen,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_2_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": user_2_seen,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification_1 = repository
            .find_delivered(id, user_1_id)
            .await?
            .expect("Fixture notification not found");
        let notification_2 = repository
            .find_delivered(id, user_2_id)
            .await?
            .expect("Fixture notification not found");

        assert_eq!(notification_1.seen, user_1_seen);
        assert_eq!(notification_2.seen, user_2_seen);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_correct_content_type() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        let content_type = "My-Custom-Content-Encoding".to_string();

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-02-10 08:46:12.123 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": &content_type,
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?.unwrap();

        assert_eq!(notification.content_type, content_type);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_correct_content() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        let content = b"My-Custom-Content".to_vec();

        collection
            .insert_many(
                [
                doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-02-10 08:46:12.123 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: content.clone(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-02-10 08:47:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?.unwrap();

        assert_eq!(notification.content, content);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_multicast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_1_id = Uuid::from_u128(1);
        let user_2_id = Uuid::from_u128(2);
        let other_user_id = Uuid::from_u128(58120381209381);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(user_1_id),
                        bson::Uuid::from(user_2_id),
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_1_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 11:11:11 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_2_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 11:11:11 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                }])
            .await?;

        let notification_1 = repository
            .find_delivered(id, user_1_id)
            .await?
            .expect("did not find notification");
        let notification_2 = repository
            .find_delivered(id, user_2_id)
            .await?
            .expect("did not find notification");
        assert_eq!(notification_1.id, notification_2.id);

        let notification_3 = repository.find_delivered(id, other_user_id).await?;
        assert!(notification_3.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_broadcast_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 11:11:11 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_some());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_notification_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;

        let notification = repository
            .find_delivered(ObjectId::new(), Uuid::from_u128(8301923019))
            .await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_other_user_notification() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);
        let other_user_id = Uuid::from_u128(48021830912);
        assert_ne!(other_user_id, user_id);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(other_user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(other_user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:38:42 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_confirmation_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(85190238019);

        collection
            .insert_many([doc! {
                "_id": id,
                "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                "invalidate_at": None as Option<DateTime>,
                "user_ids": [bson::Uuid::from(user_id)],
                "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": Binary {
                    subtype: BinarySubtype::Generic,
                    bytes: b"content of a notification".to_vec(),
                },
                "confirmations": []
            }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_broadcast_notification_confirmation_not_exist() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(481290381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:08:12 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(58102381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:09:25 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:08:12 UTC)),
                            "notification_seen": true,
                            "notification_deleted": true,
                        }
                    ]
                }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_delivered_broadcast_notification_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let id = ObjectId::new();
        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [doc! {
                    "_id": id,
                    "created_at": DateTime::from(datetime!(2024-01-28 11:02:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(481290381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:08:12 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:51:18 UTC)),
                            "notification_seen": true,
                            "notification_deleted": true,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(58102381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 12:09:25 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                }])
            .await?;

        let notification = repository.find_delivered(id, user_id).await?;

        assert!(notification.is_none());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_correct_user_id() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);
        let pagination = input::Pagination {
            page_idx: 0,
            page_size: u32::MAX,
        };
        let filters = input::NotificationFilters { seen: None };

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(481290381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(58102381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:12:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(Uuid::from_u128(4012983018923))],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"yet another notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(4012983018923)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:13:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        let notifications = repository
            .find_many_delivered(user_id, pagination, filters)
            .await?;
        assert_eq!(notifications.len(), 2);

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .unwrap();
            let user_ids = document.get_array("user_ids")?;
            let contains_user_id = user_ids
                .iter()
                .filter_map(|bson| {
                    let Bson::Binary(binary) = bson else {
                        return None;
                    };
                    let Ok(document_user_id) = binary.to_uuid() else {
                        return None;
                    };
                    Some(Uuid::from(document_user_id))
                })
                .any(|document_user_id| document_user_id == user_id);

            if !user_ids.is_empty() && !contains_user_id {
                panic!("found notification that does not belong to the user");
            }
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_skip_not_delivered() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);
        let pagination = input::Pagination {
            page_idx: 0,
            page_size: u32::MAX,
        };
        let filters = input::NotificationFilters { seen: None };

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(481290381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(58102381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:12:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"broadcast notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(123102893123)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 07:13:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-30 05:12:40 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 4,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"not delivered notification".to_vec(),
                    },
                    "confirmations": []
                },
            ])
            .await?;

        let notifications = repository
            .find_many_delivered(user_id, pagination, filters)
            .await?;
        assert_eq!(notifications.len(), 2);

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .expect("did not find id in database");
            let confirmations = document.get_array("confirmations")?;
            let contains_user_confirmation = confirmations
                .into_iter()
                .filter_map(|bson| {
                    let Bson::Document(document) = bson else {
                        return None;
                    };
                    let Some(document_user_id) = document.get("user_id") else {
                        return None;
                    };
                    let Bson::Binary(binary) = document_user_id else {
                        return None;
                    };
                    let Ok(document_user_id) = binary.to_uuid() else {
                        return None;
                    };

                    Some(Uuid::from(document_user_id))
                })
                .any(|confirmation_user_id| confirmation_user_id == user_id);

            assert_eq!(contains_user_confirmation, true);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_skip_deleted() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);
        let pagination = input::Pagination {
            page_idx: 0,
            page_size: u32::MAX,
        };
        let filters = input::NotificationFilters { seen: None };

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"content of a notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(481290381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": true,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(58102381029)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 16:12:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        let notifications = repository
            .find_many_delivered(user_id, pagination, filters)
            .await?;
        assert_eq!(notifications.len(), 1);

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .expect("did not find id in database");
            let confirmations = document.get_array("confirmations")?;
            let user_confirmation = confirmations
                .into_iter()
                .filter_map(|bson| {
                    let Bson::Document(document) = bson else {
                        return None;
                    };
                    let Some(confirmation_user_id) = document.get("user_id") else {
                        return None;
                    };
                    let Bson::Binary(binary) = confirmation_user_id else {
                        return None;
                    };
                    let Ok(confirmation_user_id) = binary.to_uuid() else {
                        return None;
                    };
                    match Uuid::from(confirmation_user_id) == user_id {
                        true => Some(document),
                        false => None,
                    }
                })
                .next()
                .expect("foud notification without user confirmation");

            let deleted = user_confirmation.get_bool("notification_deleted")?;

            assert_eq!(deleted, false);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_sorted_by_created_at_desc() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);
        let pagination = input::Pagination {
            page_idx: 0,
            page_size: u32::MAX,
        };
        let filters = input::NotificationFilters { seen: None };

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 00:12:45 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-30 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-30 16:08:12 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 16:12:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        let notifications = repository
            .find_many_delivered(user_id, pagination, filters)
            .await?;
        assert_eq!(notifications.len(), 3);

        let mut zip = std::iter::zip(
            &notifications[..notifications.len() - 1],
            &notifications[1..],
        );

        let order_correct = zip.all(|(a, b)| a.created_at >= b.created_at);

        assert!(order_correct);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_correct_pagination() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 00:12:45 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-30 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-30 16:08:12 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 16:08:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 16:12:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        let pagination_all = input::Pagination {
            page_idx: 0,
            page_size: u32::MAX,
        };
        let notifications = repository
            .find_many_delivered(
                user_id,
                pagination_all,
                input::NotificationFilters { seen: None },
            )
            .await?;
        assert_eq!(notifications.len(), 3);

        let pagination_first_two = input::Pagination {
            page_idx: 0,
            page_size: 2,
        };
        let notifications_first_two = repository
            .find_many_delivered(
                user_id,
                pagination_first_two,
                input::NotificationFilters { seen: None },
            )
            .await?;

        let pagination_last_one = input::Pagination {
            page_idx: 1,
            page_size: 2,
        };
        let notifications_last_one = repository
            .find_many_delivered(
                user_id,
                pagination_last_one,
                input::NotificationFilters { seen: None },
            )
            .await?;

        let first_two_correct = std::iter::zip(&notifications[0..2], &notifications_first_two)
            .all(|(a, b)| a.id == b.id);
        assert!(first_two_correct);

        let last_one_correct =
            std::iter::zip(&notifications[2..], &notifications_last_one).all(|(a, b)| a.id == b.id);
        assert!(last_one_correct);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_delivered_filters_seen() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(4819208301);

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 17:37:15 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 17:38:21 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        test(&repository, user_id, true).await?;
        test(&repository, user_id, false).await?;

        destroy_test_database(database).await;

        return Ok(());

        async fn test(
            repository: &NotificationsRepositoryImpl,
            user_id: Uuid,
            seen: bool,
        ) -> anyhow::Result<()> {
            let notifications = repository
                .find_many_delivered(
                    user_id,
                    input::Pagination {
                        page_idx: 0,
                        page_size: u32::MAX,
                    },
                    input::NotificationFilters { seen: Some(seen) },
                )
                .await?;
            if notifications.is_empty() {
                panic!("notifications empty");
            }

            let all_correct_seen = notifications
                .iter()
                .all(|notification| notification.seen == seen);

            match all_correct_seen {
                true => Ok(()),
                false => Err(anyhow!("filter seen: {seen} does not work")),
            }
        }
    }

    #[tokio::test]
    async fn find_many_delivered_contains_uni_multi_and_broadcast_notifications(
    ) -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(123120831);

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 17:37:15 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 17:38:21 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        },
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-28 00:12:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(Uuid::from_u128(95123123912831)),
                        bson::Uuid::from(user_id),
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-28 17:38:21 UTC)),
                            "notification_seen": false,
                            "notification_deleted": false,
                        },
                    ]
                },
            ])
            .await?;

        let notifications = repository
            .find_many_delivered(
                user_id,
                input::Pagination {
                    page_idx: 0,
                    page_size: u32::MAX,
                },
                input::NotificationFilters { seen: None },
            )
            .await?;

        assert_eq!(notifications.len(), 3);

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_undelivered_multicast_notifications() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_1_id = Uuid::from_u128(1);
        let user_2_id = Uuid::from_u128(2);
        let other_user_id = Uuid::from_u128(5981023810293);

        collection
            .insert_many([
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(user_1_id),
                        bson::Uuid::from(user_2_id),
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(user_1_id),
                        bson::Uuid::from(user_2_id),
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
            ])
            .await?;

        let notifications_1 = repository.find_many_undelivered(user_1_id).await?;
        let notifications_2 = repository.find_many_undelivered(user_2_id).await?;

        let notifications_3 = repository.find_many_undelivered(other_user_id).await?;

        assert_eq!(notifications_1.len(), 2);
        assert_eq!(notifications_2.len(), 2);

        assert!(notifications_3.is_empty());

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_undelivered_broadcast_notifications_correct_user_ids() -> anyhow::Result<()>
    {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);
        let user_id = Uuid::from_u128(4120831209);
        let other_user_id = Uuid::from_u128(580912830);
        assert_ne!(other_user_id, user_id);

        collection
            .insert_many([
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(other_user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 19:00:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
            ])
            .await?;

        let notifications = repository.find_many_undelivered(user_id).await?;
        assert_eq!(notifications.len(), 2);

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .expect("did not find id in database");
            let user_ids = document.get_array("user_ids")?;

            assert!(user_ids.is_empty());
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_undelivered_skip_notifications_with_confirmations() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(5801238012)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(48120381209)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 4,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(Uuid::from_u128(57102380129831)),
                        bson::Uuid::from(user_id)
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 5,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(57102380129831)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [
                        bson::Uuid::from(Uuid::from_u128(57102380129831)),
                        bson::Uuid::from(user_id)
                    ],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 6,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": [
                        {
                            "user_id": bson::Uuid::from(user_id),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        },
                        {
                            "user_id": bson::Uuid::from(Uuid::from_u128(57102380129831)),
                            "notification_delivered_at": DateTime::from(datetime!(2024-01-29 19:17:00 UTC)),
                            "notification_seen": true,
                            "notification_deleted": false,
                        }
                    ]
                },
            ])
            .await?;

        let notifications = repository.find_many_undelivered(user_id).await?;
        assert_eq!(notifications.len(), 3);

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .expect("did not find id in database");
            let confirmations = document.get_array("confirmations")?;
            let contains_user_confirmation = confirmations
                .into_iter()
                .filter_map(|bson| {
                    let Bson::Document(document) = bson else {
                        return None;
                    };
                    let Some(document_user_id) = document.get("user_id") else {
                        return None;
                    };
                    let Bson::Binary(binary) = document_user_id else {
                        return None;
                    };
                    let Ok(document_user_id) = binary.to_uuid() else {
                        return None;
                    };

                    Some(Uuid::from(document_user_id))
                })
                .any(|confirmation_user_id| confirmation_user_id == user_id);

            assert_eq!(contains_user_confirmation, false);
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_undelivered_skip_invalidated_notifications() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);

        collection
            .insert_many(
                [
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": DateTime::from(datetime!(9999-12-31 00:00:00 UTC)),
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": DateTime::from(OffsetDateTime::now_utc() - Duration::from_secs(6000)),
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 3,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 13:56:41 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 4,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": DateTime::from(datetime!(9999-12-31 00:00:00 UTC)),
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 5,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:52:42 UTC)),
                    "invalidate_at": DateTime::from(OffsetDateTime::now_utc() - Duration::from_secs(6000)),
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 6,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
            ])
            .await?;

        let notifications = repository.find_many_undelivered(user_id).await?;
        assert_eq!(notifications.len(), 4);

        let now = OffsetDateTime::now_utc();

        for notification in notifications {
            let document = collection
                .find_one(doc! { "_id": notification.id })
                .await?
                .expect("did not find id in database");
            let invalidate_at = document
                .get("invalidate_at")
                .expect("document has no property invalidate_at");

            if let Some(datetime) = invalidate_at.as_datetime() {
                let datetime = OffsetDateTime::from(*datetime);
                assert!(datetime >= now);
            }
        }

        destroy_test_database(database).await;

        Ok(())
    }

    #[tokio::test]
    async fn find_many_undelivered_sorted_by_created_at_asc() -> anyhow::Result<()> {
        let database = create_test_database().await?;
        let repository = NotificationsRepositoryImpl::new(database.clone()).await?;
        let collection = database.collection::<Document>(NOTIFICATIONS);

        let user_id = Uuid::from_u128(1);

        collection
            .insert_many([
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 19:57:22 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 1,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 18:22:11 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 2,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 17:15:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 4,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"notification".to_vec(),
                    },
                    "confirmations": []
                },
                doc! {
                    "created_at": DateTime::from(datetime!(2024-01-29 20:00:00 UTC)),
                    "invalidate_at": None as Option<DateTime>,
                    "user_ids": [bson::Uuid::from(user_id)],
                    "producer_id": bson::Uuid::from(Uuid::from_u128(48129038210)),
                    "producer_notification_id": 5,
                    "content_type": "utf-8",
                    "content": Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: b"other notification".to_vec(),
                    },
                    "confirmations": []
                },
            ])
            .await?;

        let notifications = repository.find_many_undelivered(user_id).await?;
        assert_eq!(notifications.len(), 4);

        let correct_order = std::iter::zip(
            &notifications[..notifications.len() - 1],
            &notifications[1..],
        )
        .all(|(a, b)| a.created_at <= b.created_at);

        assert_eq!(correct_order, true);

        destroy_test_database(database).await;

        Ok(())
    }
}

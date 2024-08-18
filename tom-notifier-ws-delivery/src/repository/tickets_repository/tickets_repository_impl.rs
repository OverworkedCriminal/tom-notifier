use super::{
    entity::{TicketFindEntity, TicketInsertEntity},
    Ticket, TicketsRepository,
};
use crate::repository::{self, Error};
use axum::async_trait;
use bson::{doc, oid::ObjectId, Bson, DateTime, Document};
use mongodb::{
    error::{ErrorKind, WriteFailure},
    options::IndexOptions,
    Database, IndexModel,
};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

const TICKETS: &str = "tickets";
const INDEX_NAME_UNIQUE_TICKET: &str = "unique_ticket";

pub struct TicketsRepositoryImpl {
    database: Database,
}

impl TicketsRepositoryImpl {
    pub async fn new(database: Database) -> Result<Self, mongodb::error::Error> {
        tracing::debug!(collection = TICKETS, "creating collection");
        database.create_collection(TICKETS).await?;

        let collection = database.collection::<Document>(TICKETS);

        tracing::debug!("fetching index names");
        let index_names = collection.list_index_names().await?;

        if !index_names.contains(&INDEX_NAME_UNIQUE_TICKET.to_string()) {
            collection
                .create_index(
                    IndexModel::builder()
                        .keys(doc! {
                            "ticket": 1,
                        })
                        .options(
                            IndexOptions::builder()
                                .name(INDEX_NAME_UNIQUE_TICKET.to_string())
                                .unique(true)
                                .build(),
                        )
                        .build(),
                )
                .await?;
            tracing::debug!(
                collection = TICKETS,
                index = INDEX_NAME_UNIQUE_TICKET,
                "created index"
            );
        }

        Ok(Self { database })
    }
}

#[async_trait]
impl TicketsRepository for TicketsRepositoryImpl {
    async fn insert(
        &self,
        ticket: &str,
        user_id: Uuid,
        issued_at: OffsetDateTime,
        expire_at: OffsetDateTime,
    ) -> Result<ObjectId, repository::Error> {
        let insert_entity = TicketInsertEntity {
            ticket,
            user_id: user_id.into(),
            issued_at: issued_at.into(),
            expire_at: expire_at.into(),
            used_at: None,
        };

        let insert_result = self
            .database
            .collection::<TicketInsertEntity>(TICKETS)
            .insert_one(insert_entity)
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

        match insert_result.inserted_id {
            Bson::ObjectId(id) => Ok(id),
            _ => Err(Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("invalid type of returned id")).into(),
            )),
        }
    }

    async fn find(&self, ticket: &str) -> Result<Option<Ticket>, repository::Error> {
        let entity = self
            .database
            .collection::<TicketFindEntity>(TICKETS)
            .find_one(doc! {
                "ticket": ticket,
            })
            .await?
            .map(Ticket::from);

        Ok(entity)
    }

    async fn update_used_at(
        &self,
        id: ObjectId,
        used_at: OffsetDateTime,
    ) -> Result<(), repository::Error> {
        let update_result = self
            .database
            .collection::<Document>(TICKETS)
            .update_one(
                doc! {
                    "_id": id,
                    "used_at": None as Option<DateTime>,
                },
                doc! {
                    "$set": {
                        "used_at": Some(DateTime::from(used_at)),
                    }
                },
            )
            .await?;

        match update_result.matched_count == 1 {
            true => Ok(()),
            false => Err(Error::NoDocumentUpdated),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use repository::tickets_repository::test::{create_test_database, destroy_test_database};
    use std::{sync::Once, time::Duration};

    static BEFORE_ALL: Once = Once::new();

    fn init_env_variables() {
        let _ = dotenvy::dotenv();
    }

    #[tokio::test]
    async fn insert_values_unchanged() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let ticket = "my very unique ticket";
        let user_id = Uuid::new_v4();
        let mut issued_at = OffsetDateTime::now_utc();
        let mut expire_at = issued_at + Duration::from_secs(30);

        let id = repository
            .insert(ticket, user_id, issued_at, expire_at)
            .await
            .unwrap();

        let entity = db
            .collection::<TicketFindEntity>(TICKETS)
            .find_one(doc! { "_id": id })
            .await
            .unwrap()
            .unwrap();

        issued_at = issued_at
            .replace_millisecond(issued_at.millisecond())
            .unwrap();
        expire_at = expire_at
            .replace_millisecond(expire_at.millisecond())
            .unwrap();

        assert_eq!(entity.ticket, ticket);
        assert_eq!(Uuid::from(entity.user_id), user_id);
        assert_eq!(OffsetDateTime::from(entity.issued_at), issued_at);
        assert_eq!(OffsetDateTime::from(entity.expire_at), expire_at);
        assert_eq!(entity.used_at, None);

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn insert_unique_ticket() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let ticket = "my very unique ticket";

        repository
            .insert(
                ticket,
                Uuid::new_v4(),
                OffsetDateTime::now_utc(),
                OffsetDateTime::now_utc() + Duration::from_secs(30),
            )
            .await
            .unwrap();

        let err = repository
            .insert(
                ticket,
                Uuid::new_v4(),
                OffsetDateTime::now_utc() + Duration::from_secs(50),
                OffsetDateTime::now_utc() + Duration::from_secs(80),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, repository::Error::InsertUniqueViolation));

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn find_exist() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let ticket = "my super important ticket";

        db.collection::<Document>(TICKETS)
            .insert_one(doc! {
                "ticket": ticket,
                "user_id": bson::Uuid::from(Uuid::new_v4()),
                "issued_at": DateTime::from(OffsetDateTime::now_utc()),
                "expire_at": DateTime::from(OffsetDateTime::now_utc() + Duration::from_secs(30)),
                "used_at": None as Option<DateTime>,
            })
            .await
            .unwrap();

        let ticket = repository.find(ticket).await.unwrap();

        assert!(ticket.is_some());

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn find_not_exist() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let ticket = repository.find("ticket that does not exist").await.unwrap();

        assert!(ticket.is_none());

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn update_used_at_value_changed() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let insert_result = db
            .collection::<Document>(TICKETS)
            .insert_one(doc! {
                "ticket": "any ticket will do",
                "user_id": bson::Uuid::from(Uuid::new_v4()),
                "issued_at": DateTime::from(OffsetDateTime::now_utc()),
                "expire_at": DateTime::from(OffsetDateTime::now_utc() + Duration::from_secs(30)),
                "used_at": None as Option<DateTime>,
            })
            .await
            .unwrap();
        let Bson::ObjectId(id) = insert_result.inserted_id else {
            panic!("invalid id type");
        };

        let mut used_at = OffsetDateTime::now_utc();

        repository.update_used_at(id, used_at).await.unwrap();

        let entity = db
            .collection::<TicketFindEntity>(TICKETS)
            .find_one(doc! { "_id": id })
            .await
            .unwrap()
            .unwrap();

        used_at = used_at.replace_millisecond(used_at.millisecond()).unwrap();

        assert_eq!(OffsetDateTime::from(entity.used_at.unwrap()), used_at);

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn update_used_at_already_non_null() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let insert_result = db
            .collection::<Document>(TICKETS)
            .insert_one(doc! {
                "ticket": "any ticket will do",
                "user_id": bson::Uuid::from(Uuid::new_v4()),
                "issued_at": DateTime::from(OffsetDateTime::now_utc()),
                "expire_at": DateTime::from(OffsetDateTime::now_utc() + Duration::from_secs(30)),
                "used_at": Some(DateTime::from(OffsetDateTime::now_utc())),
            })
            .await
            .unwrap();
        let Bson::ObjectId(id) = insert_result.inserted_id else {
            panic!("invalid id type");
        };

        let err = repository
            .update_used_at(id, OffsetDateTime::now_utc())
            .await
            .unwrap_err();

        assert!(matches!(err, Error::NoDocumentUpdated));

        destroy_test_database(db).await;
    }

    #[tokio::test]
    async fn update_used_at_not_exist() {
        BEFORE_ALL.call_once(init_env_variables);

        let db = create_test_database().await;
        let repository = TicketsRepositoryImpl::new(db.clone()).await.unwrap();

        let err = repository
            .update_used_at(ObjectId::new(), OffsetDateTime::now_utc())
            .await
            .unwrap_err();

        assert!(matches!(err, Error::NoDocumentUpdated));

        destroy_test_database(db).await;
    }
}

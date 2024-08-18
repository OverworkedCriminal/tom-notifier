use super::{TicketsSerivce, TicketsServiceConfig};
use crate::{
    dto::{input, output},
    error::Error,
    repository::{self, Ticket, TicketsRepository},
};
use axum::async_trait;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct TicketsServiceImpl {
    config: TicketsServiceConfig,
    repository: Arc<dyn TicketsRepository>,
}

impl TicketsServiceImpl {
    pub fn new(config: TicketsServiceConfig, repository: Arc<dyn TicketsRepository>) -> Self {
        Self { config, repository }
    }
}

#[async_trait]
impl TicketsSerivce for TicketsServiceImpl {
    ///
    /// Creates ticket that is used to establish WebSocket connection with the server
    ///
    /// ### Returns
    /// [output::WebSocketTicket]
    ///
    async fn create_ticket(&self, user_id: Uuid) -> Result<output::WebSocketTicket, Error> {
        tracing::info!("creating ticket");

        let issued_at = OffsetDateTime::now_utc();
        let expire_at = issued_at + self.config.ticket_lifespan;
        let ticket = Uuid::new_v4().to_string();

        let id = self
            .repository
            .insert(&ticket, user_id, issued_at, expire_at)
            .await?;
        tracing::info!(%id, "created ticket");

        Ok(output::WebSocketTicket { ticket })
    }

    ///
    /// Consumes ticket to receive information about the user
    ///
    /// ### Returns
    /// [Ticket] with user information
    ///
    /// ### Errors
    /// - [Error::TicketInvalid] when
    ///     - ticket does not exist
    ///     - ticket had already been used
    ///     - ticket expired
    ///
    async fn consume_ticket(
        &self,
        input::WebSocketTicket { ticket }: input::WebSocketTicket,
    ) -> Result<Ticket, Error> {
        tracing::info!("consuming ticket");

        let mut ticket = self
            .repository
            .find(&ticket)
            .await?
            .ok_or(Error::TicketInvalid("ticket not exist"))?;

        if ticket.used_at.is_some() {
            return Err(Error::TicketInvalid("ticket already used"));
        }

        let now = OffsetDateTime::now_utc();
        if ticket.expire_at < now {
            return Err(Error::TicketInvalid("ticket expired"));
        }

        match self.repository.update_used_at(ticket._id, now).await {
            Ok(()) => {
                tracing::info!(id = %ticket._id, "consumed ticket");
                ticket.used_at = Some(now);
                Ok(ticket)
            }
            Err(repository::Error::NoDocumentUpdated) => {
                Err(Error::TicketInvalid("ticket already used"))
            }
            Err(err) => Err(Error::Database(err)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bson::oid::ObjectId;
    use repository::MockTicketsRepository;
    use std::time::Duration;

    #[tokio::test]
    async fn create_ticket_unique_tickets_returned() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository
            .expect_insert()
            .returning(|_, _, _, _| Ok(ObjectId::new()));
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let user_id = Uuid::new_v4();
        let ticket_1 = service.create_ticket(user_id).await.unwrap();
        let ticket_2 = service.create_ticket(user_id).await.unwrap();

        assert_ne!(ticket_1.ticket, ticket_2.ticket);
    }

    #[tokio::test]
    async fn create_ticket_insert_unique_violation() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository
            .expect_insert()
            .returning(|_, _, _, _| Err(repository::Error::InsertUniqueViolation));
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let create_result = service.create_ticket(Uuid::new_v4()).await;

        assert!(matches!(create_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn create_ticket_database_error() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_insert().returning(|_, _, _, _| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let create_result = service.create_ticket(Uuid::new_v4()).await;

        assert!(matches!(create_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn consume_ticket_not_exist() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|_| Ok(None));
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::TicketInvalid(_))));
    }

    #[tokio::test]
    async fn consume_ticket_find_database_error() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|_| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn consume_ticket_already_used() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|ticket| {
            Ok(Some(Ticket {
                _id: ObjectId::new(),
                ticket: ticket.to_string(),
                user_id: Uuid::new_v4(),
                issued_at: OffsetDateTime::now_utc() - Duration::from_secs(60),
                expire_at: OffsetDateTime::now_utc() + Duration::from_secs(30),
                used_at: Some(OffsetDateTime::now_utc() - Duration::from_secs(40)),
            }))
        });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::TicketInvalid(_))));
    }

    #[tokio::test]
    async fn consume_ticket_already_expired() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|ticket| {
            Ok(Some(Ticket {
                _id: ObjectId::new(),
                ticket: ticket.to_string(),
                user_id: Uuid::new_v4(),
                issued_at: OffsetDateTime::now_utc() - Duration::from_secs(60),
                expire_at: OffsetDateTime::now_utc() - Duration::from_secs(30),
                used_at: None,
            }))
        });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::TicketInvalid(_))));
    }

    #[tokio::test]
    async fn consume_ticket_used_at_updated() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let before_update = OffsetDateTime::now_utc();
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|ticket| {
            Ok(Some(Ticket {
                _id: ObjectId::new(),
                ticket: ticket.to_string(),
                user_id: Uuid::new_v4(),
                issued_at: OffsetDateTime::now_utc() - Duration::from_secs(60),
                expire_at: OffsetDateTime::now_utc() + Duration::from_secs(60),
                used_at: None,
            }))
        });
        repository
            .expect_update_used_at()
            .returning(move |_, used_at| {
                let after_update = OffsetDateTime::now_utc();
                assert!(before_update <= used_at && used_at <= after_update);
                Ok(())
            });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await
            .unwrap();

        // assertion happen in mock
    }

    #[tokio::test]
    async fn consume_ticket_no_document_updated() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|ticket| {
            Ok(Some(Ticket {
                _id: ObjectId::new(),
                ticket: ticket.to_string(),
                user_id: Uuid::new_v4(),
                issued_at: OffsetDateTime::now_utc() - Duration::from_secs(60),
                expire_at: OffsetDateTime::now_utc() + Duration::from_secs(60),
                used_at: None,
            }))
        });
        repository
            .expect_update_used_at()
            .returning(|_, _| Err(repository::Error::NoDocumentUpdated));
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::TicketInvalid(_))));
    }

    #[tokio::test]
    async fn consume_ticket_update_database_error() {
        let config = TicketsServiceConfig {
            ticket_lifespan: Duration::from_secs(30),
        };
        let mut repository = MockTicketsRepository::new();
        repository.expect_find().returning(|ticket| {
            Ok(Some(Ticket {
                _id: ObjectId::new(),
                ticket: ticket.to_string(),
                user_id: Uuid::new_v4(),
                issued_at: OffsetDateTime::now_utc() - Duration::from_secs(60),
                expire_at: OffsetDateTime::now_utc() + Duration::from_secs(60),
                used_at: None,
            }))
        });
        repository.expect_update_used_at().returning(|_, _| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = TicketsServiceImpl::new(config, Arc::new(repository));

        let consume_result = service
            .consume_ticket(input::WebSocketTicket {
                ticket: "my ticket".to_string(),
            })
            .await;

        assert!(matches!(consume_result, Err(Error::Database(_))));
    }
}

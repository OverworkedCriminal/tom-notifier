mod error;
mod tickets_repository;

pub use error::*;
pub use tickets_repository::*;

#[cfg(test)]
mod test {
    use crate::application::ApplicationEnv;
    use mongodb::{options::ClientOptions, Client, Database};
    use uuid::Uuid;

    pub async fn create_test_database() -> Database {
        let env = ApplicationEnv::parse().unwrap();
        let db_name = format!("test_{}", Uuid::new_v4());

        println!("creating test database: {db_name}");

        let db_client_options = ClientOptions::parse(env.db_connection_string)
            .await
            .unwrap();
        let db_client = Client::with_options(db_client_options).unwrap();
        let db = db_client.database(&db_name);

        db
    }

    pub async fn destroy_test_database(database: Database) {
        let _ = database.drop().await;
        database.client().clone().shutdown().await;
    }
}

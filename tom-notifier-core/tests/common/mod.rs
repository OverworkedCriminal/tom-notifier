use jwt_auth::test::create_jwt;
use std::sync::Once;
use uuid::Uuid;

static INIT_ENV_ONCE: Once = Once::new();

pub fn init_env() {
    INIT_ENV_ONCE.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

pub fn address() -> String {
    std::env::var("TOM_NOTIFIER_CORE_BIND_ADDRESS").unwrap()
}

pub fn create_producer_jwt_with_id(user_id: Uuid) -> String {
    encode_jwt(user_id, &["tom_notifier_produce_notifications"])
}

pub fn create_producer_jwt() -> String {
    let user_id = Uuid::new_v4();
    create_producer_jwt_with_id(user_id)
}

pub fn create_consumer_jwt_with_id(user_id: Uuid) -> String {
    encode_jwt(user_id, &[])
}

pub fn create_consumer_jwt() -> String {
    let user_id = Uuid::new_v4();
    encode_jwt(user_id, &[])
}

fn encode_jwt(user_id: Uuid, roles: &[&str]) -> String {
    let jwt_algorithms = std::env::var("TOM_NOTIFIER_CORE_JWT_ALGORITHMS").unwrap();
    let jwt_key = std::env::var("TOM_NOTIFIER_CORE_JWT_TEST_ENCODE_KEY").unwrap();

    create_jwt(user_id, roles, jwt_algorithms, jwt_key)
}

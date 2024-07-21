use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use std::str::FromStr;
use uuid::Uuid;

pub fn address() -> String {
    std::env::var("TOM_NOTIFIER_CORE_BIND_ADDRESS").unwrap()
}

pub fn create_producer_bearer() -> String {
    let jwt = create_producer_jwt();
    let bearer = format!("Bearer {jwt}");

    bearer
}

pub fn create_consumer_bearer() -> String {
    let jwt = create_consumer_jwt();
    let bearer = format!("Bearer {jwt}");

    bearer
}

pub fn create_producer_jwt_with_id(user_id: Uuid) -> String {
    let claims = json!({
        "sub": user_id,
        "exp": 253402210800_i64,
        "realm_access": {
            "roles": [ "produce_notifications" ]
        }
    });

    let jwt = encode_jwt(claims);

    jwt
}

pub fn create_producer_jwt() -> String {
    let user_id = Uuid::new_v4();
    create_producer_jwt_with_id(user_id)
}

pub fn create_consumer_jwt_with_id(user_id: Uuid) -> String {
    let claims = json!({
        "sub": user_id,
        "exp": 253402210800_i64,
        "realm_access": {
            "roles": []
        }
    });

    let jwt = encode_jwt(claims);

    jwt
}

pub fn create_consumer_jwt() -> String {
    let user_id = Uuid::new_v4();
    create_consumer_jwt_with_id(user_id)
}

fn encode_jwt(claims: Value) -> String {
    let jwt_algorithm = parse_env_jwt_algorithm();
    let jwt_key = parse_env_jwt_key(&jwt_algorithm);

    let jwt = jsonwebtoken::encode(&Header::new(jwt_algorithm), &claims, &jwt_key).unwrap();

    jwt
}

fn parse_env_jwt_algorithm() -> Algorithm {
    let jwt_algorithms = std::env::var("TOM_NOTIFIER_CORE_JWT_ALGORITHMS").unwrap();

    let mut algorithms = Vec::new();
    for algorithm_str in jwt_algorithms.split(',') {
        let algorithm = Algorithm::from_str(algorithm_str).unwrap();
        algorithms.push(algorithm);
    }
    let Some(jwt_algorithm) = algorithms.first() else {
        panic!("TOM_NOTIFIER_CORE_JWT_ALGORITHMS does not contain any algorithm");
    };

    *jwt_algorithm
}

fn parse_env_jwt_key(jwt_algorithm: &Algorithm) -> EncodingKey {
    let jwt_key_string = std::env::var("TOM_NOTIFIER_CORE_JWT_TEST_ENCODE_KEY").unwrap();
    let jwt_key_bytes = jwt_key_string.as_bytes();
    let jwt_key = match jwt_algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            EncodingKey::from_secret(jwt_key_bytes)
        }
        Algorithm::ES256 | Algorithm::ES384 => EncodingKey::from_ec_pem(jwt_key_bytes).unwrap(),
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            EncodingKey::from_rsa_pem(jwt_key_bytes).unwrap()
        }
        Algorithm::PS256 | Algorithm::PS384 | Algorithm::PS512 => {
            EncodingKey::from_rsa_pem(jwt_key_bytes).unwrap()
        }
        Algorithm::EdDSA => EncodingKey::from_ec_pem(jwt_key_bytes).unwrap(),
    };

    jwt_key
}

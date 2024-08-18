#![cfg(feature = "test_utils")]

use crate::util::parse_jwt_algorithms;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn create_jwt(
    user_id: Uuid,
    roles: &[&str],
    jwt_algorithms: String,
    jwt_key: String,
) -> String {
    let claims = json!({
        "sub": user_id,
        "exp": 253402210800_i64,
        "realm_access": {
            "roles": roles,
        }
    });

    let jwt = encode_jwt(claims, jwt_algorithms, jwt_key);

    jwt
}

fn encode_jwt(claims: Value, jwt_algorithms: String, jwt_key: String) -> String {
    let jwt_algorithms = parse_jwt_algorithms(jwt_algorithms).unwrap();
    let jwt_algorithm = jwt_algorithms
        .first()
        .expect("algorithms list cannot be empty");
    let jwt_key = parse_jwt_encoding_key(jwt_algorithm, jwt_key);

    let jwt = jsonwebtoken::encode(&Header::new(*jwt_algorithm), &claims, &jwt_key).unwrap();

    jwt
}

fn parse_jwt_encoding_key(jwt_algorithm: &Algorithm, key: String) -> EncodingKey {
    let jwt_key_bytes = key.as_bytes();
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

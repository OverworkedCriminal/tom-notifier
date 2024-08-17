use anyhow::anyhow;
use jsonwebtoken::{Algorithm, DecodingKey};
use std::str::FromStr;

pub fn parse_jwt_algorithms(jwt_algorithms: String) -> anyhow::Result<Vec<Algorithm>> {
    let mut algorithms = Vec::new();

    for algorithm_str in jwt_algorithms.split(',') {
        let algorithm = Algorithm::from_str(algorithm_str)
            .map_err(|err| anyhow!("invalid algorithm: {err}"))?;
        algorithms.push(algorithm);
    }

    Ok(algorithms)
}

pub fn parse_jwt_key(jwt_algorithm: &Algorithm, jwt_key: String) -> anyhow::Result<DecodingKey> {
    let jwt_key_bytes = jwt_key.as_bytes();

    let key = match jwt_algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            DecodingKey::from_secret(jwt_key_bytes)
        }
        Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(jwt_key_bytes)
            .map_err(|err| anyhow!("invalid ec pem key: {err}"))?,
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            DecodingKey::from_rsa_pem(jwt_key_bytes)
                .map_err(|err| anyhow!("invalid rsa pem key: {err}"))?
        }
        Algorithm::PS256 | Algorithm::PS384 | Algorithm::PS512 => {
            DecodingKey::from_rsa_pem(jwt_key_bytes)
                .map_err(|err| anyhow!("invalid rsa pem key: {err}"))?
        }
        Algorithm::EdDSA => DecodingKey::from_ec_pem(jwt_key_bytes)
            .map_err(|err| anyhow!("invalid ec pem key: {err}"))?,
    };

    Ok(key)
}

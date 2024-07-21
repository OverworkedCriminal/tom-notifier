use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct JwtClaims {
    pub sub: Uuid,
    pub exp: i64,
    pub realm_access: JwtClaimsRealmAccess,
}

#[derive(Deserialize)]
pub struct JwtClaimsRealmAccess {
    pub roles: Vec<String>,
}

use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub realm_access: JwtClaimsRealmAccess,
}

#[derive(Deserialize)]
pub struct JwtClaimsRealmAccess {
    pub roles: Vec<String>,
}

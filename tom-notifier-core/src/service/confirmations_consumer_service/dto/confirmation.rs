use crate::dto::input;
use anyhow::anyhow;
use bson::oid::ObjectId;
use std::str::FromStr;
use uuid::Uuid;

pub struct Confirmation {
    pub user_id: Uuid,
    pub id: ObjectId,
}

impl TryFrom<input::ConfirmationProtobuf> for Confirmation {
    type Error = anyhow::Error;

    fn try_from(value: input::ConfirmationProtobuf) -> Result<Self, Self::Error> {
        let user_id =
            Uuid::from_str(&value.user_id).map_err(|err| anyhow!("invalid user_id: {err}"))?;
        let id = ObjectId::from_str(&value.id).map_err(|err| anyhow!("invalid id: {err}"))?;

        Ok(Self { user_id, id })
    }
}

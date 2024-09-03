use crate::dto::input;
use anyhow::anyhow;
use bson::oid::ObjectId;
use time::OffsetDateTime;

pub struct NotificationStatusUpdate {
    pub id: ObjectId,
    pub timestamp: OffsetDateTime,
}

impl TryFrom<&input::NotificationProtobuf> for NotificationStatusUpdate {
    type Error = anyhow::Error;

    fn try_from(value: &input::NotificationProtobuf) -> Result<Self, Self::Error> {
        let id = ObjectId::parse_str(&value.id)
            .map_err(|err| anyhow!("notification invalid: invalid object id: {err}"))?;
        let timestamp = value
            .timestamp
            .ok_or_else(|| anyhow!("notification invalid: missing timestamp field"))?;
        let timestamp = OffsetDateTime::from_unix_timestamp(timestamp.seconds)
            .map_err(|err| {
                anyhow!("notification invalid: invalid timestamp: invalid seconds: {err}")
            })?
            .replace_nanosecond(timestamp.nanos as u32)
            .map_err(|err| {
                anyhow!("notification invalid: invalid timestamp: invalid nanos: {err}")
            })?;

        Ok(Self { id, timestamp })
    }
}

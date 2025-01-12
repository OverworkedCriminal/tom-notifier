use crate::repository;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Serialize)]
pub struct Notification {
    pub id: String,
    pub created_at: OffsetDateTime,
    pub created_by: Uuid,
    pub seen: bool,
    pub content_type: String,
    #[serde(with = "se_base64")]
    pub content: Vec<u8>,
}

mod se_base64 {
    use base64::{prelude::BASE64_STANDARD, Engine};
    use serde::{Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = BASE64_STANDARD.encode(v);
        let result = String::serialize(&base64, s);

        result
    }
}

impl From<repository::Notification> for Notification {
    fn from(value: repository::Notification) -> Self {
        Self {
            id: value.id.to_hex(),
            created_at: value.created_at,
            created_by: value.producer_id.into(),
            seen: value.seen,
            content_type: value.content_type,
            content: value.content,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use base64::{prelude::BASE64_STANDARD, Engine};
    use serde_json::Value;
    use time::OffsetDateTime;

    #[test]
    fn notification_json_serialize_ok() {
        let content = b"my bytes".to_vec();
        let notification = Notification {
            id: "1".to_string(),
            created_at: OffsetDateTime::now_utc(),
            created_by: Uuid::new_v4(),
            seen: false,
            content_type: "utf-8".to_string(),
            content: content.clone(),
        };

        let json = serde_json::to_string(&notification).unwrap();

        let object = serde_json::from_str::<Value>(&json).unwrap();
        let json_content = object
            .as_object()
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(json_content, BASE64_STANDARD.encode(content))
    }
}

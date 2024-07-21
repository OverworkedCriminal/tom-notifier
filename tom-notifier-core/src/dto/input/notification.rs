use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct Notification {
    pub invalidate_at: Option<OffsetDateTime>,
    pub user_ids: Vec<Uuid>,
    pub producer_notification_id: i64,
    pub content_type: String,
    #[serde(with = "de_base64")]
    pub content: Vec<u8>,
}

mod de_base64 {
    //!
    //! Module allows to deserialize JSON base64 string directly
    //! to bytes, so it's not neccessary to do it in services
    //!

    use base64::{prelude::BASE64_STANDARD, Engine};
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let string = String::deserialize(d)?;
        let bytes = BASE64_STANDARD
            .decode(string)
            .map_err(|e| serde::de::Error::custom(e))?;

        Ok(bytes)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use base64::prelude::*;

    #[test]
    fn notification_json_deserialize_ok() {
        let content = "MTIzNA==";
        let json = format!(
            r#"{{
                "invalidate_at": null,
                "user_ids": [],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "{content}"
            }}"#
        );

        let notification = serde_json::from_str::<Notification>(&json).unwrap();

        assert_eq!(
            notification.content,
            BASE64_STANDARD.decode(content).unwrap()
        );
    }

    #[test]
    fn notification_json_deserialize_base64_invalid() {
        let json = r#"{
            "invalidate_at": null,
            "user_ids": [],
            "producer_notification_id": 1,
            "content_type": "utf-8",
            "content": "¢≠³² ¢²≠³≠²¢12"
        }"#;

        let notification = serde_json::from_str::<Notification>(&json);

        assert!(notification.is_err());
    }
}

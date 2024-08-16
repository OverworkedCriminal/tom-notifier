# Tom-Notifier-Core

Main part of tom-notifier system.

#### Main features
- creating notifications
- deleting notifications
- distinction between notifications that were delivered and not
- updating `seen` state of delivered notifications
- (uni/multi/broad)cast notifications
- expiring notifications 
(notification is not delivered to the user if `invalidate_at` timestamp has passed)
- RabbitMQ integration
    - producing - following endpoints send message to `TOM_NOTIFIER_CORE_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME` exchange with `routing_key`
    `NEW`/`UPDATED`/`DELETED`
        - POST `/api/v1/notifications/undelivered`
        - PUT `/api/v1/notifications/delivered/:notification_id/seen`
        - DELETE `/api/v1/notifications/delivered/:notification_id`
        
    - consuming - confirmations published to `TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME` exchange are consumed from
    `TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_QUEUE_NAME` queue to mark undelivered
    notifications as delivered



## Endpoints



### POST `/api/v1/notifications/undelivered`
Create new notification.

Empty `user_ids` creates broadcast notification
#### Body
```
{
    invalidate_at: Option<OffsetDateTime>,
    user_ids: Vec<Uuid>,
    producer_notification_id: i64,
    content_type: String,
    content: String,
}
```
#### Response on success
```
{
    id: String,
}
```
#### Response Code
| Status code | when? |
| --- | --- |
| 200 | success |
| 400 | content field is not valid base64 |
| 403 | user lacks role `tom_notifier_produce_notifications` |
| 409 | notification with producer_notification_id already exist |
| 413 | content is too large |
| 422 | invalidate_at is set to past date |




### GET `/api/v1/notifications/undelivered`
Fetch list of all undelivered notifications.
Since this endpoint delivers notifications, they are marked as delivered.
It means fetching undelivered notification multiple times will yield
different results.

This endpoint can be used for long polling new notifications
#### Response on success
```
[
    {
        id: String,
        created_at: OffsetDateTime,
        created_by: Uuid,
        seen: bool,
        content_type: String,
        content: String,
    },
    ...
]
```
#### Response Code
| Status code | when? |
| --- | --- |
| 200 | success |




### PUT `/api/v1/notifications/undelivered/:notification_id/invalidate_at`
Update invalidate_at property of the notification
#### Path
| param | description|
| --- | --- |
| notification_id | hex form of ObjectId |

#### Body
```
{
    invalidate_at: Option<OffsetDateTime>,
}
```
#### Response Code
| Status code | when? |
| --- | --- |
| 204 | success |
| 403 | user does not have role `tom_notifier_produce_notifications` |
| 404 | - notification does not exist <br> - notification was created by different user |
| 422 | invalidate_at is set to date that have already passed |




### GET `/api/v1/notifications/delivered`
Fetch list of delivered notifications
#### Params
| param | description|
| --- | --- |
| page_idx | indexing starts at 0 |
| page_size | |
| seen | optional parameter that allows filtering by `seen` property |

#### Response on success
```
[
    {
        id: String,
        created_at: OffsetDateTime,
        created_by: Uuid,
        seen: bool,
        content_type: String,
        content: String,
    },
    ...
]
```

#### Response Code
| Status code | when? |
| --- | --- |
| 200 | success |




### GET `/api/v1/notifications/delivered/:notification_id`
Fetch delivered notification
#### Path
| param | description|
| --- | --- |
| notification_id | hex form of ObjectId |

#### Response on success
```
{
    id: String,
    created_at: OffsetDateTime,
    created_by: Uuid,
    seen: bool,
    content_type: String,
    content: String,
}
```

#### Response Code
| Status code | when? |
| --- | --- |
| 200 | success |
| 404 | - notification does not exist <br> - user does not belong to notification recipients <br> - notification has not been delivered yet <br> - notification is deleted |




### DELETE `/api/v1/notifications/delivered/:notification_id`
Delete notification
#### Path
| param | description|
| --- | --- |
| notification_id | hex form of ObjectId |

#### Response Code
| Status code | when? |
| --- | --- |
| 204 | success |
| 404 | - notification does not exist <br> - user does not belong to notification recipients <br> - notification has not been delivered yet <br> - notification is deleted |




### PUT `/api/v1/notifications/delivered/:notification_id/seen`
Update `seen` property of the notification
#### Path
| param | description|
| --- | --- |
| notification_id | hex form of ObjectId |
#### Body
```
{
    seen: bool,
}
```

#### Response Code
| Status code | when? |
| --- | --- |
| 204 | success |
| 404 | - notification does not exist <br> - user does not belong to notification recipients <br> - notification has not been delivered yet <br> - notification is deleted |
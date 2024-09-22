use std::{future::Future, time::Duration};

///
/// Run async function in a loop until it returns Ok
///
pub async fn retry<AttemptF, ErrF, F, Fut, T>(
    retry_interval: Duration,
    attempt_log_fn: AttemptF,
    error_log_fn: ErrF,
    async_fn: F,
) -> T
where
    AttemptF: Fn(u32),
    ErrF: Fn(u32, amqprs::error::Error),
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, amqprs::error::Error>>,
{
    let mut attempt = 0;

    loop {
        attempt += 1;

        attempt_log_fn(attempt);
        match async_fn().await {
            Ok(output) => return output,
            Err(err) => error_log_fn(attempt, err),
        }

        tokio::time::sleep(retry_interval).await;
    }
}

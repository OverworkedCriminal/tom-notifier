#
# Dockerfile is meant to be build with the context set to the parent directory
#

FROM rust:1.81 as builder

WORKDIR /usr/src/tom-notifier-core

COPY ./tom-notifier-core ./
COPY ./shared ../shared

RUN apt-get update && apt-get install -y protobuf-compiler && cargo install --path .



FROM debian:bookworm-slim

ENV TOM_NOTIFIER_CORE_LOG_DIRECTORY="logs"
ENV TOM_NOTIFIER_CORE_LOG_FILENAME="tom-notifier-core-log"
ENV TOM_NOTIFIER_CORE_BIND_ADDRESS="0.0.0.0:4000"
ENV TOM_NOTIFIER_CORE_DB_CONNECTION_STRING="mongodb://admin:admin@localhost:27017"
ENV TOM_NOTIFIER_CORE_DB_NAME="tom_notifier_core"
ENV TOM_NOTIFIER_CORE_MAX_NOTIFICATION_CONTENT_LEN="4096"
ENV TOM_NOTIFIER_CORE_MAX_HTTP_CONTENT_LEN="8192"
ENV TOM_NOTIFIER_CORE_JWT_ALGORITHMS="HS256,HS512"
ENV TOM_NOTIFIER_CORE_JWT_KEY="secret"
ENV TOM_NOTIFIER_CORE_RABBITMQ_CONNECTION_STRING="amqp://admin:admin@localhost:5672"
ENV TOM_NOTIFIER_CORE_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME="tom_notifier_notifications"
ENV TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME="tom_notifier_confirmations"
ENV TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_QUEUE_NAME="tom_notifier_confirmations"
ENV TOM_NOTIFIER_CORE_RABBITMQ_RETRY_INTERVAL="10"

RUN apt-get update && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/tom-notifier-core /usr/local/bin/tom-notifier-core

CMD [ "tom-notifier-core" ]
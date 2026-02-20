# SDK Contract v2.5 Topics Domain

Status: Draft, Release B target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.topics`
2. `sdk.capability.topic_subscriptions`
3. `sdk.capability.topic_fanout`

## SDK Trait Surface

1. `topic_create`
2. `topic_get`
3. `topic_list`
4. `topic_subscribe`
5. `topic_unsubscribe`
6. `topic_publish`

## Core Types

1. `TopicId`
2. `TopicPath`
3. `TopicCreateRequest`
4. `TopicRecord`
5. `TopicListRequest`
6. `TopicListResult`
7. `TopicSubscriptionRequest`
8. `TopicPublishRequest`

## Rules

1. Topic identity is canonical `topic_id`; `topic_path` is optional metadata.
2. Topic publish requires either `sdk.capability.topic_fanout` or backend-specific equivalent.
3. Topic list operations are cursor-based and must return deterministic ordering.

mod support;

use anyhow::{Context, Result};
use chrono::Utc;
use grammers_client::message::{InputMessage, Message};
use grammers_client::sender::SenderPool;
use grammers_client::Client;
use grammers_session::storages::MemorySession;
use grammers_session::types::{PeerAuth, PeerId, PeerRef};
use grammers_tl_types as tl;
use sea_orm::{ActiveValue::NotSet, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use std::sync::Arc;
use support::{init_test_tracing, TestDatabase};
use telegram_indexer::sync::MessageProcessor;

use entities::telegram::prelude::{TelegramChannels, TelegramMessages};
use entities::telegram::{telegram_channels, telegram_messages};

#[tokio::test]
async fn processor_backfills_reply_links_and_keeps_upserts_idempotent() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    let now = Utc::now().naive_utc();
    let channel = TelegramChannels::insert(telegram_channels::ActiveModel {
        id: NotSet,
        telegram_id: Set(-1007000000001),
        name: Set("processor-test-channel".to_string()),
        username: Set(Some("processor_test".to_string())),
        r#type: Set("channel".to_string()),
        created_at: Set(now),
        updated_at: Set(now),
    })
    .exec_with_returning(conn.as_ref())
    .await
    .context("failed to create channel fixture")?;

    let processor = MessageProcessor::new(conn.clone());
    let client = synthetic_client();
    let peer = PeerRef {
        id: PeerId::channel(7000000001),
        auth: PeerAuth::default(),
    };

    let base_timestamp = Utc::now().timestamp() as i32;
    let reply = synthetic_message(
        &client,
        peer,
        2,
        "second message that replies to the first",
        Some(1),
        base_timestamp + 1,
    );
    processor
        .process_message(&reply, channel.id)
        .await
        .context("processing reply message should succeed")?;

    let reply_row_before = TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel.id))
        .filter(telegram_messages::Column::TelegramMessageId.eq(2_i64))
        .one(conn.as_ref())
        .await?
        .context("reply message should be present")?;

    assert_eq!(reply_row_before.reply_to_telegram_message_id, Some(1));
    assert_eq!(reply_row_before.reply_to_message_id, None);

    let parent = synthetic_message(
        &client,
        peer,
        1,
        "first parent message",
        None,
        base_timestamp,
    );
    processor
        .process_message(&parent, channel.id)
        .await
        .context("processing parent message should succeed")?;

    let parent_row = TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel.id))
        .filter(telegram_messages::Column::TelegramMessageId.eq(1_i64))
        .one(conn.as_ref())
        .await?
        .context("parent message should be present")?;

    let reply_row_after = TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel.id))
        .filter(telegram_messages::Column::TelegramMessageId.eq(2_i64))
        .one(conn.as_ref())
        .await?
        .context("reply message should still be present")?;

    assert_eq!(reply_row_after.reply_to_telegram_message_id, Some(1));
    assert_eq!(reply_row_after.reply_to_message_id, Some(parent_row.id));

    processor
        .process_message(&parent, channel.id)
        .await
        .context("reprocessing parent should upsert, not duplicate")?;

    let total_messages = TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel.id))
        .count(conn.as_ref())
        .await?;
    assert_eq!(total_messages, 2);

    Ok(())
}

fn synthetic_client() -> Client {
    let session = Arc::new(MemorySession::default());
    let sender_pool = SenderPool::new(session, 1);
    Client::new(sender_pool.handle)
}

fn synthetic_message(
    client: &Client,
    peer: PeerRef,
    message_id: i32,
    text: &str,
    reply_to: Option<i32>,
    timestamp: i32,
) -> Message {
    let updates = tl::types::UpdateShortSentMessage {
        out: false,
        id: message_id,
        pts: message_id,
        pts_count: 1,
        date: timestamp,
        media: None,
        entities: None,
        ttl_period: None,
    };

    let input = InputMessage::new().text(text).reply_to(reply_to);
    Message::from_raw_short_updates(client, updates, input, peer)
}

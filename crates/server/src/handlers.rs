use crate::{auth::MaybeUser, authz, db::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use common::{
    AddReactionRequest, Channel, ChannelId, CreateChannelRequest, CreateMessageRequest,
    CreateReplyRequest, Message, MessageId, MessageReply, Reaction, UserId, WsServerEvent,
};
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, to_bson, Bson},
    options::FindOptions,
    Collection,
};
use serde::Deserialize;
use uuid::Uuid;

fn channels(state: &AppState) -> Collection<Channel> {
    state.db.collection("channels")
}

fn messages(state: &AppState) -> Collection<Message> {
    state.db.collection("messages")
}

#[derive(Debug, Deserialize, Default)]
pub struct ListMessagesQuery {
    pub limit: Option<i64>,
}

pub async fn list_channels(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> Result<Json<Vec<Channel>>, (StatusCode, String)> {
    ensure_allowed(user.as_ref().map(|u| u.id), authz::Action::ReadChannels)?;
    Ok(Json(list_channels_core(&state).await?))
}

pub async fn create_channel(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Json(req): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<Channel>), (StatusCode, String)> {
    ensure_allowed(user.as_ref().map(|u| u.id), authz::Action::CreateChannel)?;
    let channel = create_channel_core(&state, req).await?;
    Ok((StatusCode::CREATED, Json(channel)))
}

pub async fn list_messages(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(channel_id): Path<ChannelId>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<Message>>, (StatusCode, String)> {
    ensure_allowed(user.as_ref().map(|u| u.id), authz::Action::ReadMessages)?;
    Ok(Json(list_messages_core(&state, channel_id, query.limit).await?))
}

pub async fn create_message(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(channel_id): Path<ChannelId>,
    Json(req): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<Message>), (StatusCode, String)> {
    let actor = resolve_actor(user.as_ref().map(|u| u.id), req.author_id)?;
    ensure_allowed(Some(actor), authz::Action::SendMessage)?;
    let message = create_message_core(&state, channel_id, actor, req.body).await?;
    Ok((StatusCode::CREATED, Json(message)))
}

pub async fn create_reply(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((channel_id, parent_id)): Path<(ChannelId, MessageId)>,
    Json(req): Json<CreateReplyRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    let actor = resolve_actor(user.as_ref().map(|u| u.id), req.author_id)?;
    ensure_allowed(Some(actor), authz::Action::ReplyMessage)?;
    let (root_message_id, reply) =
        create_reply_core(&state, channel_id, parent_id, actor, req.body).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"root_message_id": root_message_id, "reply": reply})),
    ))
}

pub async fn add_reaction(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((channel_id, message_id)): Path<(ChannelId, MessageId)>,
    Json(req): Json<AddReactionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let actor = resolve_actor(user.as_ref().map(|u| u.id), req.user_id)?;
    ensure_allowed(Some(actor), authz::Action::React)?;
    let reactions = add_reaction_core(&state, channel_id, message_id, actor, req.emoji).await?;
    Ok(Json(
        serde_json::json!({"message_id": message_id, "reactions": reactions}),
    ))
}

pub async fn list_channels_core(state: &AppState) -> Result<Vec<Channel>, (StatusCode, String)> {
    let cursor = channels(state).find(doc! {}).await.map_err(internal_err)?;
    let mut items: Vec<Channel> = cursor.try_collect().await.map_err(internal_err)?;
    items.sort_by_key(|c| c.created_at);
    Ok(items)
}

pub async fn create_channel_core(
    state: &AppState,
    req: CreateChannelRequest,
) -> Result<Channel, (StatusCode, String)> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let channel = Channel {
        id: Uuid::new_v4(),
        name,
        channel_type: req.channel_type,
        created_at: Utc::now(),
    };
    channels(state)
        .insert_one(&channel)
        .await
        .map_err(internal_err)?;
    state
        .hub
        .publish(WsServerEvent::ChannelCreated(channel.clone()));
    Ok(channel)
}

pub async fn list_messages_core(
    state: &AppState,
    channel_id: ChannelId,
    limit: Option<i64>,
) -> Result<Vec<Message>, (StatusCode, String)> {
    let opts = match limit {
        Some(limit) => FindOptions::builder()
            .sort(doc! { "created_at": 1 })
            .limit(limit.clamp(1, 500))
            .build(),
        None => FindOptions::builder()
            .sort(doc! { "created_at": 1 })
            .build(),
    };
    let cursor = messages(state)
        .find(doc! { "channel_id": uuid_bson(channel_id)? })
        .with_options(opts)
        .await
        .map_err(internal_err)?;
    let items: Vec<Message> = cursor.try_collect().await.map_err(internal_err)?;
    Ok(items)
}

pub async fn create_message_core(
    state: &AppState,
    channel_id: ChannelId,
    author_id: UserId,
    body: String,
) -> Result<Message, (StatusCode, String)> {
    if !channel_exists(state, channel_id).await? {
        return Err((StatusCode::NOT_FOUND, "channel not found".into()));
    }
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "body is required".into()));
    }
    let message = Message {
        id: Uuid::new_v4(),
        channel_id,
        author_id,
        body,
        created_at: Utc::now(),
        edited_at: None,
        replies: vec![],
        reactions: vec![],
    };
    messages(state)
        .insert_one(&message)
        .await
        .map_err(internal_err)?;
    state
        .hub
        .publish(WsServerEvent::MessageCreated(message.clone()));
    Ok(message)
}

pub async fn create_reply_core(
    state: &AppState,
    channel_id: ChannelId,
    parent_id: MessageId,
    author_id: UserId,
    body: String,
) -> Result<(MessageId, MessageReply), (StatusCode, String)> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "body is required".into()));
    }

    let mut root = find_root_for_message(state, channel_id, parent_id)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "parent message not found".into()))?;

    let reply = MessageReply {
        id: Uuid::new_v4(),
        parent_id,
        author_id,
        body,
        created_at: Utc::now(),
        edited_at: None,
        replies: vec![],
        reactions: vec![],
    };

    let inserted = if root.id == parent_id {
        root.replies.push(reply.clone());
        true
    } else {
        insert_reply(&mut root.replies, parent_id, reply.clone())
    };
    if !inserted {
        return Err((StatusCode::NOT_FOUND, "parent message not found".into()));
    }

    replace_root(state, &root).await?;
    state.hub.publish(WsServerEvent::ReplyCreated {
        channel_id,
        root_message_id: root.id,
        reply: reply.clone(),
    });
    Ok((root.id, reply))
}

pub async fn add_reaction_core(
    state: &AppState,
    channel_id: ChannelId,
    message_id: MessageId,
    user_id: UserId,
    emoji: String,
) -> Result<Vec<Reaction>, (StatusCode, String)> {
    let emoji = emoji.trim().to_string();
    if emoji.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "emoji is required".into()));
    }
    let mut root = find_root_for_message(state, channel_id, message_id)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "message not found".into()))?;

    let reactions = if root.id == message_id {
        toggle_reaction(&mut root.reactions, &emoji, user_id)
    } else if let Some(r) = toggle_reply_reaction(&mut root.replies, message_id, &emoji, user_id) {
        r
    } else {
        return Err((StatusCode::NOT_FOUND, "message not found".into()));
    };

    replace_root(state, &root).await?;
    state.hub.publish(WsServerEvent::ReactionUpdated {
        channel_id,
        message_id,
        reactions: reactions.clone(),
    });
    Ok(reactions)
}

pub fn resolve_actor(session_user: Option<UserId>, provided: Option<UserId>) -> Result<UserId, (StatusCode, String)> {
    if let Some(user_id) = session_user {
        return Ok(user_id);
    }
    if let Some(user_id) = provided {
        return Ok(user_id);
    }
    Err((
        StatusCode::UNAUTHORIZED,
        "user identity required (authenticate or provide author_id/user_id)".into(),
    ))
}

pub fn ensure_allowed(user_id: Option<UserId>, action: authz::Action) -> Result<(), (StatusCode, String)> {
    if authz::allow(user_id, action) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "forbidden".into()))
    }
}

async fn channel_exists(state: &AppState, channel_id: ChannelId) -> Result<bool, (StatusCode, String)> {
    let found = channels(state)
        .find_one(doc! { "id": uuid_bson(channel_id)? })
        .await
        .map_err(internal_err)?;
    Ok(found.is_some())
}

async fn find_root_for_message(
    state: &AppState,
    channel_id: ChannelId,
    message_id: MessageId,
) -> Result<Option<Message>, (StatusCode, String)> {
    let cursor = messages(state)
        .find(doc! { "channel_id": uuid_bson(channel_id)? })
        .await
        .map_err(internal_err)?;
    let roots: Vec<Message> = cursor.try_collect().await.map_err(internal_err)?;
    Ok(roots
        .into_iter()
        .find(|m| m.id == message_id || contains_reply(&m.replies, message_id)))
}

async fn replace_root(state: &AppState, root: &Message) -> Result<(), (StatusCode, String)> {
    messages(state)
        .replace_one(doc! { "id": uuid_bson(root.id)? }, root)
        .await
        .map_err(internal_err)?;
    Ok(())
}

fn insert_reply(replies: &mut Vec<MessageReply>, parent_id: MessageId, reply: MessageReply) -> bool {
    for r in replies.iter_mut() {
        if r.id == parent_id {
            r.replies.push(reply);
            return true;
        }
        if insert_reply(&mut r.replies, parent_id, reply.clone()) {
            return true;
        }
    }
    false
}

fn contains_reply(replies: &[MessageReply], id: MessageId) -> bool {
    replies
        .iter()
        .any(|r| r.id == id || contains_reply(&r.replies, id))
}

fn toggle_reaction(reactions: &mut Vec<Reaction>, emoji: &str, user_id: uuid::Uuid) -> Vec<Reaction> {
    if let Some(existing) = reactions.iter_mut().find(|r| r.emoji == emoji) {
        if let Some(pos) = existing.user_ids.iter().position(|u| *u == user_id) {
            existing.user_ids.remove(pos);
            if existing.user_ids.is_empty() {
                reactions.retain(|r| r.emoji != emoji);
            }
        } else {
            existing.user_ids.push(user_id);
        }
    } else {
        reactions.push(Reaction {
            emoji: emoji.to_string(),
            user_ids: vec![user_id],
        });
    }
    reactions.clone()
}

fn toggle_reply_reaction(
    replies: &mut [MessageReply],
    message_id: MessageId,
    emoji: &str,
    user_id: uuid::Uuid,
) -> Option<Vec<Reaction>> {
    for r in replies.iter_mut() {
        if r.id == message_id {
            return Some(toggle_reaction(&mut r.reactions, emoji, user_id));
        }
        if let Some(res) = toggle_reply_reaction(&mut r.replies, message_id, emoji, user_id) {
            return Some(res);
        }
    }
    None
}

fn uuid_bson(id: uuid::Uuid) -> Result<Bson, (StatusCode, String)> {
    to_bson(&id).map_err(internal_err)
}

fn internal_err<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

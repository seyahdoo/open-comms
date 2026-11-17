use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type UserId = Uuid;
pub type ChannelId = Uuid;
pub type MessageId = Uuid;
pub type SessionId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub channel_type: ChannelType,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub token: String,
    pub user_id: UserId,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub user: User,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Reaction {
    pub emoji: String,
    pub user_ids: Vec<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub author_id: UserId,
    pub body: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub edited_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub replies: Vec<MessageReply>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReply {
    pub id: MessageId,
    pub parent_id: MessageId,
    pub author_id: UserId,
    pub body: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub edited_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub replies: Vec<MessageReply>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum WsClientEvent {
    Ping,
    SendMessage {
        channel_id: ChannelId,
        #[serde(default)]
        author_id: Option<UserId>,
        body: String,
    },
    ReplyMessage {
        channel_id: ChannelId,
        parent_id: MessageId,
        #[serde(default)]
        author_id: Option<UserId>,
        body: String,
    },
    AddReaction {
        channel_id: ChannelId,
        message_id: MessageId,
        #[serde(default)]
        user_id: Option<UserId>,
        emoji: String,
    },
    JoinVoice {
        channel_id: ChannelId,
        sdp: Value,
    },
    RtcAnswer {
        sdp: Value,
    },
    IceCandidate {
        candidate: Value,
    },
    LeaveVoice {
        channel_id: ChannelId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum WsServerEvent {
    Pong,
    Error { message: String },
    ChannelCreated(Channel),
    MessageCreated(Message),
    ReplyCreated { channel_id: ChannelId, root_message_id: MessageId, reply: MessageReply },
    ReactionUpdated { channel_id: ChannelId, message_id: MessageId, reactions: Vec<Reaction> },
    VoiceJoined { channel_id: ChannelId },
    VoiceLeft { channel_id: ChannelId },
    RtcOffer { sdp: Value },
    RtcAnswer { sdp: Value },
    IceCandidate { candidate: Value },
    VoiceError { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: ChannelType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    #[serde(default)]
    pub author_id: Option<UserId>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReplyRequest {
    #[serde(default)]
    pub author_id: Option<UserId>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddReactionRequest {
    #[serde(default)]
    pub user_id: Option<UserId>,
    pub emoji: String,
}

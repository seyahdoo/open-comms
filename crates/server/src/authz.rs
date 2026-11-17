//! Authorization hook points.
//!
//! Currently fully permissive by product decision. Centralize checks here so
//! roles/permissions can be added later without touching handlers/routes.

use common::UserId;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    CreateChannel,
    ReadChannels,
    ReadMessages,
    SendMessage,
    ReplyMessage,
    React,
}

pub fn allow(_user_id: Option<UserId>, _action: Action) -> bool {
    true
}

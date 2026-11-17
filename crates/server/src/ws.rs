use crate::db::AppState;
use axum::extract::ws::{Message, WebSocket};
use common::{UserId, WsClientEvent, WsServerEvent};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::warn;

use crate::handlers;

pub async fn handle_socket(socket: WebSocket, state: AppState, session_user: Option<UserId>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut hub_rx = state.hub.subscribe();
    let out_tx_broadcast = out_tx.clone();
    let mut broadcast_task = tokio::spawn(async move {
        while let Ok(event) = hub_rx.recv().await {
            if send_json(&out_tx_broadcast, &*event).is_err() {
                break;
            }
        }
    });

    let out_tx_recv = out_tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Err(err_event) =
                        handle_text(&state, &out_tx_recv, session_user, &text).await
                    {
                        let _ = send_json(&out_tx_recv, &err_event);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => {
            broadcast_task.abort();
            recv_task.abort();
        }
        _ = &mut broadcast_task => {
            send_task.abort();
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
            broadcast_task.abort();
        }
    }
}

async fn handle_text(
    state: &AppState,
    out_tx: &mpsc::UnboundedSender<Message>,
    session_user: Option<UserId>,
    text: &str,
) -> Result<(), WsServerEvent> {
    let event: WsClientEvent = serde_json::from_str(text).map_err(|e| WsServerEvent::Error {
        message: format!("invalid event: {e}"),
    })?;

    match event {
        WsClientEvent::Ping => {
            let _ = send_json(out_tx, &WsServerEvent::Pong);
            Ok(())
        }
        WsClientEvent::SendMessage {
            channel_id,
            author_id,
            body,
        } => {
            let actor = handlers::resolve_actor(session_user, author_id)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::ensure_allowed(Some(actor), crate::authz::Action::SendMessage)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::create_message_core(state, channel_id, actor, body)
                .await
                .map(|_| ())
                .map_err(|(_, msg)| WsServerEvent::Error { message: msg })
        }
        WsClientEvent::ReplyMessage {
            channel_id,
            parent_id,
            author_id,
            body,
        } => {
            let actor = handlers::resolve_actor(session_user, author_id)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::ensure_allowed(Some(actor), crate::authz::Action::ReplyMessage)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::create_reply_core(state, channel_id, parent_id, actor, body)
                .await
                .map(|_| ())
                .map_err(|(_, msg)| WsServerEvent::Error { message: msg })
        }
        WsClientEvent::AddReaction {
            channel_id,
            message_id,
            user_id,
            emoji,
        } => {
            let actor = handlers::resolve_actor(session_user, user_id)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::ensure_allowed(Some(actor), crate::authz::Action::React)
                .map_err(|(_, message)| WsServerEvent::Error { message })?;
            handlers::add_reaction_core(state, channel_id, message_id, actor, emoji)
                .await
                .map(|_| ())
                .map_err(|(_, msg)| WsServerEvent::Error { message: msg })
        }
    }
}

fn send_json(tx: &mpsc::UnboundedSender<Message>, event: &WsServerEvent) -> Result<(), ()> {
    match serde_json::to_string(event) {
        Ok(text) => tx.send(Message::Text(text.into())).map_err(|_| ()),
        Err(err) => {
            warn!("failed to serialize ws event: {err}");
            Err(())
        }
    }
}

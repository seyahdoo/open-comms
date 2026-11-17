use crate::db::AppState;
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use chrono::Utc;
use common::{AuthResponse, RegisterRequest, User, UserId};
use mongodb::{bson::doc, Collection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub token: String,
    pub user_id: UserId,
    pub created_at: chrono::DateTime<Utc>,
}

fn users(state: &AppState) -> Collection<User> {
    state.db.collection("users")
}

fn sessions(state: &AppState) -> Collection<SessionRecord> {
    state.db.collection("sessions")
}

pub async fn register_user(
    state: &AppState,
    req: RegisterRequest,
) -> Result<AuthResponse, (StatusCode, String)> {
    let username = req.username.trim().to_string();
    if username.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "username is required".into()));
    }

    let user = if let Some(existing) = users(state)
        .find_one(doc! { "username": &username })
        .await
        .map_err(internal_err)?
    {
        existing
    } else {
        let user = User {
            id: Uuid::new_v4(),
            username: username.clone(),
            created_at: Utc::now(),
        };
        users(state)
            .insert_one(&user)
            .await
            .map_err(internal_err)?;
        user
    };

    let token = Uuid::new_v4().to_string();
    let session = SessionRecord {
        token: token.clone(),
        user_id: user.id,
        created_at: Utc::now(),
    };
    sessions(state)
        .insert_one(&session)
        .await
        .map_err(internal_err)?;

    Ok(AuthResponse { user, token })
}

pub async fn user_for_token(state: &AppState, token: &str) -> Result<Option<User>, (StatusCode, String)> {
    let Some(session) = sessions(state)
        .find_one(doc! { "token": token })
        .await
        .map_err(internal_err)?
    else {
        return Ok(None);
    };

    let user = users(state)
        .find_one(doc! { "id": mongodb::bson::to_bson(&session.user_id).map_err(internal_err)? })
        .await
        .map_err(internal_err)?;
    Ok(user)
}

pub fn token_from_auth_header(value: &str) -> Option<&str> {
    value.strip_prefix("Bearer ").map(str::trim).filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct MaybeUser(pub Option<User>);

#[async_trait]
impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Some(header) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
            return Ok(MaybeUser(None));
        };
        let Ok(header) = header.to_str() else {
            return Ok(MaybeUser(None));
        };
        let Some(token) = token_from_auth_header(header) else {
            return Ok(MaybeUser(None));
        };
        let user = user_for_token(state, token).await?;
        Ok(MaybeUser(user))
    }
}

fn internal_err<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

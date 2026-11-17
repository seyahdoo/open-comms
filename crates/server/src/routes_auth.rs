use crate::{auth, db::AppState};
use axum::{extract::State, http::StatusCode, Json};
use common::{AuthResponse, RegisterRequest, User};

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), (StatusCode, String)> {
    let res = auth::register_user(&state, req).await?;
    Ok((StatusCode::CREATED, Json(res)))
}

pub async fn me(
    State(_state): State<AppState>,
    auth::MaybeUser(user): auth::MaybeUser,
) -> Result<Json<Option<User>>, (StatusCode, String)> {
    Ok(Json(user))
}

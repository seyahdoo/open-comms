use crate::{config::Config, hub::Hub};
use mongodb::{bson::doc, options::IndexOptions, Client, Database, IndexModel};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub hub: Hub,
}

pub async fn init_state(config: &Config) -> anyhow::Result<AppState> {
    let client = Client::with_uri_str(&config.mongodb_uri).await?;
    let db = client.database(&config.mongodb_database);
    // Touch the server to fail fast when configuration is invalid.
    db.run_command(doc! { "ping": 1 }).await?;
    ensure_indexes(&db).await?;
    Ok(AppState {
        db,
        hub: Hub::new(256),
    })
}

async fn ensure_indexes(db: &Database) -> anyhow::Result<()> {
    let channels = db.collection::<mongodb::bson::Document>("channels");
    channels
        .create_index(
            IndexModel::builder()
                .keys(doc! { "id": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;

    let messages = db.collection::<mongodb::bson::Document>("messages");
    messages
        .create_index(
            IndexModel::builder()
                .keys(doc! { "id": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;
    messages
        .create_index(
            IndexModel::builder()
                .keys(doc! { "channel_id": 1, "created_at": 1 })
                .build(),
        )
        .await?;

    let users = db.collection::<mongodb::bson::Document>("users");
    users
        .create_index(
            IndexModel::builder()
                .keys(doc! { "id": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;
    users
        .create_index(
            IndexModel::builder()
                .keys(doc! { "username": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;

    let sessions = db.collection::<mongodb::bson::Document>("sessions");
    sessions
        .create_index(
            IndexModel::builder()
                .keys(doc! { "token": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;
    sessions
        .create_index(
            IndexModel::builder()
                .keys(doc! { "user_id": 1 })
                .build(),
        )
        .await?;

    Ok(())
}

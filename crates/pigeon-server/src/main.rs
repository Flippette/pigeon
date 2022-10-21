mod eyre;

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    sync::Arc,
    time::UNIX_EPOCH,
};

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router, Server,
};
use eyre::Result;
use parking_lot::RwLock;
use serde::Deserialize;

use pigeon_server::*;

const USERS_FILE: &str = "users.json";
const MESSAGES_FILE: &str = "messages.json";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    #[cfg(debug_assertions)]
    tracing_subscriber::fmt().pretty().init();

    #[cfg(not(debug_assertions))]
    tracing_subscriber::fmt().compact().init();

    let state = {
        let users = serde_json::from_str(&fs::read_to_string(USERS_FILE).unwrap_or_else(|err| {
            tracing::warn!("Error reading {}: {}, using new userlist", USERS_FILE, err);
            String::new()
        }))
        .unwrap_or_else(|err| {
            tracing::warn!("Error parsing {}: {}, using new userlist", USERS_FILE, err);
            HashSet::new()
        });

        let messages =
            serde_json::from_str(&fs::read_to_string(MESSAGES_FILE).unwrap_or_else(|err| {
                tracing::warn!(
                    "Error reading {}: {}, using new messagelist",
                    MESSAGES_FILE,
                    err
                );
                String::new()
            }))
            .unwrap_or_else(|err| {
                tracing::warn!(
                    "Error parsing {}: {}, using new messages list",
                    MESSAGES_FILE,
                    err
                );
                BTreeMap::new()
            });

        Arc::new(RwLock::new(State { users, messages }))
    };

    let app = Router::new()
        .route("/", get(|| async { "Hello, world!" }))
        .route("/register/:username", post(register))
        .route("/message", post(send).get(recv))
        .layer(Extension(Arc::clone(&state)));

    tokio::task::spawn(
        Server::bind(&"0.0.0.0:3000".parse().unwrap()).serve(app.into_make_service()),
    );

    tokio::signal::ctrl_c().await?;

    fs::write(
        USERS_FILE,
        serde_json::to_string(&Arc::clone(&state).read().users)?,
    )?;
    fs::write(
        MESSAGES_FILE,
        serde_json::to_string(&Arc::clone(&state).read().messages)?,
    )?;

    Ok(())
}

async fn register(
    Path(username): Path<String>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<(), StatusCode> {
    if state.write().users.insert(username) {
        Ok(())
    } else {
        Err(StatusCode::CONFLICT)
    }
}

async fn send(
    Json(message): Json<Message>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<(), StatusCode> {
    if !state.read().users.contains(&message.author) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    state
        .write()
        .add_message_at_present(message)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct RecvInfo {
    username: String,
    timestamp: u64,
}

async fn recv(
    Query(recv_info): Query<RecvInfo>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<Json<Vec<(u64, Message)>>, StatusCode> {
    if !state.read().users.contains(&recv_info.username) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let current_time = UNIX_EPOCH.elapsed().unwrap().as_secs();
    Ok(Json(
        state
            .read()
            .messages
            .iter()
            .filter(|(&ts, _)| ts < current_time && ts > recv_info.timestamp)
            .flat_map(|(&ts, msgs)| msgs.iter().map(move |msg| (ts, msg.clone())))
            .collect::<Vec<_>>(),
    ))
}

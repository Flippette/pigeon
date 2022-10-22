mod eyre;

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    sync::Arc,
    time::UNIX_EPOCH,
};

use axum::{
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
const SALT: [u8; 16] = *b"Hello, world!!!!";
const BCRYPT_COST: u32 = 12;

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
            HashMap::new()
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
        .route("/register", post(register))
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

#[derive(Deserialize)]
struct RegInfo {
    username: String,
    password: String,
}

async fn register(
    Json(reg_info): Json<RegInfo>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<(), StatusCode> {
    if state.read().users.contains_key(&reg_info.username) {
        return Err(StatusCode::CONFLICT);
    }

    let hash = match bcrypt::hash_with_salt(reg_info.password, BCRYPT_COST, SALT) {
        Ok(hash) => hash,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
    .to_string();

    state.write().users.insert(reg_info.username, hash);

    Ok(())
}

#[derive(Deserialize)]
struct SendInfo {
    password: String,
    message: Message,
}

async fn send(
    Json(send_info): Json<SendInfo>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<(), StatusCode> {
    if !auth(
        &state.read(),
        &send_info.message.author,
        &send_info.password,
        BCRYPT_COST,
        SALT,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    for user in &send_info.message.recipients {
        if !state.read().users.contains_key(user) {
            return Err(StatusCode::NOT_ACCEPTABLE);
        }
    }

    state
        .write()
        .add_message_at_present(send_info.message)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct RecvInfo {
    username: String,
    password: String,
    timestamp: u64,
}

async fn recv(
    Json(recv_info): Json<RecvInfo>,
    Extension(state): Extension<Arc<RwLock<State>>>,
) -> Result<Json<Vec<(u64, Message)>>, StatusCode> {
    if !auth(
        &state.read(),
        &recv_info.username,
        &recv_info.password,
        BCRYPT_COST,
        SALT,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let current_time = UNIX_EPOCH.elapsed().unwrap().as_secs();
    Ok(Json(
        state
            .read()
            .messages
            .iter()
            .filter(|(&ts, _)| ts < current_time && ts > recv_info.timestamp)
            .flat_map(|(&ts, msgs)| {
                msgs.iter()
                    .filter(|msg| msg.recipients.contains(&recv_info.username))
                    .map(move |msg| (ts, msg.clone()))
            })
            .collect::<Vec<_>>(),
    ))
}

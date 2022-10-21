mod eyre;

use std::{
    collections::{BTreeMap, HashSet},
    time::UNIX_EPOCH,
};

use eyre::{ensure, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug)]
pub struct State {
    pub users: HashSet<String>,
    pub messages: BTreeMap<u64, Vec<Message>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub author: String,
    pub content: String,
}

#[derive(Debug, Error, Deserialize, Serialize)]
pub enum AppError {
    #[error("Message author doesn't exist!")]
    NonExistentMessageAuthor,
}

impl State {
    pub fn add_message_at_present(&mut self, message: Message) -> Result<()> {
        let timestamp = UNIX_EPOCH.elapsed()?.as_secs();
        ensure!(
            self.users.contains(&message.author),
            AppError::NonExistentMessageAuthor
        );
        self.messages
            .entry(timestamp)
            .and_modify(|messages| messages.push(message.clone()))
            .or_insert_with(|| vec![message]);
        Ok(())
    }
}

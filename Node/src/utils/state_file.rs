use super::file_lock::FileLock;
use anyhow::Error;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct State {
    pub registered: bool,
}

impl State {
    pub fn read_state_file(path: &str) -> Result<Self, Error> {
        let file_lock = FileLock::new_shared(path)?;
        let state: State = serde_json::from_reader(file_lock.get_file())?;
        Ok(state)
    }
}

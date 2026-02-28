use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::board::Board;

type HmacSha256 = Hmac<Sha256>;

const SECRET_KEY: &[u8] = b"szsol_secret_key_123_do_not_cheat";
const HMAC_SIZE: usize = 32;

/// A single recorded game session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecord {
    pub seed: u64,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub won: bool,
    pub initial_board: Option<Board>,
    pub current_board: Option<Board>,
    pub undo_history: Vec<Board>,
}

impl GameRecord {
    pub fn new(seed: u64, start_time: i64) -> Self {
        Self {
            seed,
            start_time,
            end_time: None,
            won: false,
            initial_board: None,
            current_board: None,
            undo_history: Vec::new(),
        }
    }
}

/// The entire game history.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct History {
    pub records: Vec<GameRecord>,
}

impl History {
    /// Load the history from disk. If the file doesn't exist or is corrupted/tampered,
    /// returns an empty new History to avoid crashing the game.
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };

        if !path.exists() {
            return Self::default();
        }

        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return Self::default(),
        };

        let mut data = Vec::new();
        if file.read_to_end(&mut data).is_err() {
            return Self::default();
        }

        if data.len() < HMAC_SIZE {
            // File is too small to even contain the HMAC
            return Self::default();
        }

        let split_idx = data.len() - HMAC_SIZE;
        let payload = &data[..split_idx];
        let signature = &data[split_idx..];

        // Verify HMAC
        let mut mac = match HmacSha256::new_from_slice(SECRET_KEY) {
            Ok(m) => m,
            Err(_) => return Self::default(),
        };
        mac.update(payload);
        if mac.verify_slice(signature).is_err() {
            // Tampered or corrupted file
            eprintln!("[WARN] Save file signature mismatched! Starting with fresh history.");
            return Self::default();
        }

        match bincode::deserialize(payload) {
            Ok(history) => history,
            Err(_) => Self::default(),
        }
    }

    /// Save the history to disk atomically to prevent corruption.
    pub fn save(&self) {
        let Some(path) = Self::file_path() else { return };
        
        // Ensure the directory exists
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }

        let payload = match bincode::serialize(self) {
            Ok(p) => p,
            Err(_) => return,
        };

        let mut mac = match HmacSha256::new_from_slice(SECRET_KEY) {
            Ok(m) => m,
            Err(_) => return,
        };
        mac.update(&payload);
        let signature = mac.finalize().into_bytes();

        let mut final_data = payload.clone();
        final_data.extend_from_slice(&signature);

        // Atomic write: write to temp file, then rename.
        // On Unix, `rename` is atomic. On Windows, `rename` is also mostly atomic,
        // but can fail if the target is held open. Standard Rust `fs::rename` uses `MoveFileExW` 
        // with `MOVEFILE_REPLACE_EXISTING`, which is atomic enough for this use-case.
        let mut temp_path = path.clone();
        temp_path.set_extension("tmp");

        let mut temp_file = match File::create(&temp_path) {
            Ok(f) => f,
            Err(_) => return,
        };

        if temp_file.write_all(&final_data).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }

        // Flush all OS buffers to disk before renaming to ensure data integrity
        // in case of a sudden power loss exactly during or after rename.
        if temp_file.sync_all().is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }

        let _ = fs::rename(&temp_path, &path);
    }

    /// Get the path to the save file (`history.dat`).
    fn file_path() -> Option<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "szsol", "szsol")?;
        Some(proj_dirs.data_dir().join("history.dat"))
    }
}

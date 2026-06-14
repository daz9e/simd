use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
struct SavedSession {
    username: String,
    expires_at: u64,
}

pub struct Session {
    pub username: String,
    expires_at: u64,
}

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    duration_secs: u64,
    save_path: PathBuf,
}

impl SessionStore {
    pub fn new(duration_secs: u64, save_path: PathBuf) -> Self {
        let mut store = Self {
            sessions: HashMap::new(),
            duration_secs,
            save_path,
        };
        store.load();
        store
    }

    pub const fn duration(&self) -> u64 {
        self.duration_secs
    }

    pub fn create(&mut self, username: String) -> String {
        let token = generate_token();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.cleanup();
        self.sessions.insert(
            token.clone(),
            Session {
                username,
                expires_at: now + self.duration_secs,
            },
        );
        self.save();
        token
    }

    pub fn validate(&mut self, token: &str) -> Option<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let session = self.sessions.get(token);
        match session {
            Some(s) if s.expires_at > now => Some(s.username.clone()),
            _ => {
                self.sessions.remove(token);
                self.save();
                None
            }
        }
    }

    pub fn remove(&mut self, token: &str) {
        self.sessions.remove(token);
        self.save();
    }

    fn cleanup(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let size_before = self.sessions.len();
        self.sessions.retain(|_, s| s.expires_at > now);
        if self.sessions.len() != size_before {
            self.save();
        }
    }

    fn save(&self) {
        let data: HashMap<String, SavedSession> = self
            .sessions
            .iter()
            .map(|(k, v)| {
                (k.clone(), SavedSession {
                    username: v.username.clone(),
                    expires_at: v.expires_at,
                })
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&data) {
            if let Some(parent) = self.save_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&self.save_path, json);
        }
    }

    fn load(&mut self) {
        let Ok(data) = fs::read_to_string(&self.save_path) else {
            return;
        };
        let Ok(saved): Result<HashMap<String, SavedSession>, _> = serde_json::from_str(&data) else {
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for (token, s) in saved {
            if s.expires_at > now {
                self.sessions.insert(token, Session {
                    username: s.username,
                    expires_at: s.expires_at,
                });
            }
        }
    }
}

fn generate_token() -> String {
    let mut buf = [0u8; 32];
    match fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[simd] FATAL: failed to read /dev/urandom: {e}");
            std::process::exit(1);
        }
    }
    buf.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

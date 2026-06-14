use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    pub username: String,
    expires_at: u64,
}

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    duration_secs: u64,
}

impl SessionStore {
    pub fn new(duration_secs: u64) -> Self {
        Self {
            sessions: HashMap::new(),
            duration_secs,
        }
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
                None
            }
        }
    }

    pub fn remove(&mut self, token: &str) {
        self.sessions.remove(token);
    }

    fn cleanup(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.sessions.retain(|_, s| s.expires_at > now);
    }
}

fn generate_token() -> String {
    let mut buf = [0u8; 32];
    match File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)) {
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

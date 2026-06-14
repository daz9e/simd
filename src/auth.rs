use crate::config::{AppConfig, write_config};
use crate::session::SessionStore;
use crate::AppMode;
use base64::Engine;
use bcrypt::{hash, verify, DEFAULT_COST};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const MIN_PASSWORD_LEN: usize = 8;

const DUMMY_HASH: &str =
    "$2b$12$LJ3m4ys3LkBCVxJGqOjFruRqM8Gqq.ZNT2vKzmpMvRQbKMfTn7FyK";

struct RateLimitEntry {
    failures: u32,
    blocked_until: Instant,
}

pub struct RateLimiter {
    attempts: HashMap<IpAddr, RateLimitEntry>,
    max_failures: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_failures: u32, window_secs: u64) -> Self {
        Self {
            attempts: HashMap::new(),
            max_failures,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn check(&mut self, ip: IpAddr) -> Result<(), String> {
        let now = Instant::now();
        self.attempts.retain(|_, v| v.blocked_until > now || v.failures > 0);
        if let Some(entry) = self.attempts.get(&ip) {
            if entry.blocked_until > now {
                let remaining = entry.blocked_until.duration_since(now).as_secs();
                return Err(format!("Too many attempts. Retry in {remaining}s"));
            }
        }
        Ok(())
    }

    pub fn record_failure(&mut self, ip: IpAddr) {
        let now = Instant::now();
        let entry = self.attempts.entry(ip).or_insert(RateLimitEntry {
            failures: 0,
            blocked_until: now,
        });
        entry.failures += 1;
        if entry.failures >= self.max_failures {
            let block_dur = self.window * (entry.failures / self.max_failures);
            entry.blocked_until = now + block_dur;
        }
    }

    pub fn clear(&mut self, ip: IpAddr) {
        self.attempts.remove(&ip);
    }
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub user: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub user: String,
    pub password: String,
}

pub fn get_cookie(req: &tiny_http::Request, name: &str) -> Option<String> {
    for header in req.headers() {
        if format!("{}", header.field).eq_ignore_ascii_case("Cookie") {
            for cookie in header.value.as_str().split(';') {
                let cookie = cookie.trim();
                if let Some(eq) = cookie.find('=') {
                    let key = cookie[..eq].trim();
                    let val = cookie[eq + 1..].trim();
                    if key.eq_ignore_ascii_case(name) {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn is_authenticated(
    config: &AppConfig,
    req: &tiny_http::Request,
    session_store: &Arc<Mutex<SessionStore>>,
) -> bool {
    if let Some(token) = get_cookie(req, "simd_session") {
        let mut store = session_store.lock().unwrap_or_else(PoisonError::into_inner);
        if store.validate(&token).is_some() {
            return true;
        }
    }

    check_basic_auth(config, req)
}

fn check_basic_auth(config: &AppConfig, req: &tiny_http::Request) -> bool {
    let auth_header = req.headers().iter().find(|h| {
        format!("{}", h.field).eq_ignore_ascii_case("Authorization")
    });
    let auth_value = match auth_header {
        Some(h) => h.value.as_str().to_string(),
        None => return false,
    };

    if !auth_value.starts_with("Basic ") {
        return false;
    }

    let encoded = &auth_value[6..];
    let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    let Some(colon_pos) = decoded.find(':') else { return false };

    let user = &decoded[..colon_pos];
    let password = &decoded[colon_pos + 1..];

    if user != config.user {
        let _ = verify(password, DUMMY_HASH);
        return false;
    }

    verify(password, &config.password_hash).unwrap_or(false)
}

pub fn handle_login(
    body: &str,
    config: &AppConfig,
    session_store: &Arc<Mutex<SessionStore>>,
    rate_limiter: &Arc<Mutex<RateLimiter>>,
    ip: IpAddr,
) -> Result<String, String> {
    let login: LoginRequest =
        serde_json::from_str(body).map_err(|_| "Invalid request".to_string())?;

    {
        let mut rl = rate_limiter.lock().unwrap_or_else(PoisonError::into_inner);
        rl.check(ip)?;
    }

    let hash_to_check = if login.user == config.user {
        &config.password_hash
    } else {
        DUMMY_HASH
    };

    if !verify(&login.password, hash_to_check).unwrap_or(false) || login.user != config.user {
        rate_limiter.lock().unwrap_or_else(PoisonError::into_inner).record_failure(ip);
        return Err("Invalid credentials".to_string());
    }

    rate_limiter.lock().unwrap_or_else(PoisonError::into_inner).clear(ip);

    let mut store = session_store.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(store.create(login.user))
}

pub fn handle_setup_body(
    body: &str,
    config_path: &Path,
    mode: &mut MutexGuard<'_, AppMode>,
) -> Result<String, String> {
    let setup: SetupRequest =
        serde_json::from_str(body).map_err(|_| "Invalid request".to_string())?;

    if setup.user.is_empty() || setup.password.is_empty() {
        return Err("Username and password are required".to_string());
    }

    if setup.password.len() < MIN_PASSWORD_LEN {
        return Err(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters"
        ));
    }

    let password_hash = hash(&setup.password, DEFAULT_COST)
        .map_err(|_| "Failed to hash password".to_string())?;

    let config = AppConfig {
        user: setup.user.clone(),
        password_hash,
    };

    write_config(config_path, &config)?;

    **mode = AppMode::Normal(config);

    Ok(setup.user)
}



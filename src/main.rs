mod auth;
mod cache;
mod config;
mod markdown;
mod session;
mod tree;

use config::AppConfig;
use serde::Serialize;
use std::io::Read;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub enum AppMode {
    Setup,
    Normal(AppConfig),
}

struct Ctx {
    mode: Arc<Mutex<AppMode>>,
    data_path: PathBuf,
    config_path: PathBuf,
    file_cache: Arc<Mutex<cache::FileCache>>,
    tree_cache: Arc<Mutex<cache::TreeCache>>,
    session_store: Arc<Mutex<session::SessionStore>>,
    rate_limiter: Arc<Mutex<auth::RateLimiter>>,
}

const APP_PAGE: &str = include_str!("static/index.html");
const SETUP_PAGE: &str = include_str!("static/setup.html");
const LOGIN_PAGE: &str = include_str!("static/login.html");

const MAX_BODY_SIZE: usize = 64 * 1024;
const MAX_THREADS: usize = 128;

fn main() {
    let data_dir = std::env::var("SIMD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let config_dir = std::env::var("SIMD_CONFIG_DIR").unwrap_or_else(|_| "/config".to_string());
    let port = std::env::var("SIMD_PORT").unwrap_or_else(|_| "8080".to_string());
    let session_duration: u64 = std::env::var("SIMD_SESSION_DURATION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(86400);

    let config_path = PathBuf::from(&config_dir).join("config.json");
    let data_path = PathBuf::from(&data_dir);

    let mode = config::read_config(&config_path).map_or_else(
        || {
            eprintln!("[simd] No config found. Setup mode.");
            AppMode::Setup
        },
        |c| {
            eprintln!("[simd] Config loaded from {}", config_path.display());
            AppMode::Normal(c)
        },
    );

    let mode = Arc::new(Mutex::new(mode));
    let file_cache = Arc::new(Mutex::new(cache::FileCache::new(100)));
    let tree_cache = Arc::new(Mutex::new(cache::TreeCache::new(30)));
    let session_store = Arc::new(Mutex::new(session::SessionStore::new(session_duration)));
    let rate_limiter = Arc::new(Mutex::new(auth::RateLimiter::new(5, 300)));

    let ctx = Arc::new(Ctx {
        mode,
        data_path,
        config_path,
        file_cache,
        tree_cache,
        session_store,
        rate_limiter,
    });

    let addr = format!("0.0.0.0:{port}");

    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[simd] Failed to start server on {addr}: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "[simd] Server running on http://localhost:{port} , data: {data_dir}"
    );

    let active_threads = Arc::new(AtomicUsize::new(0));

    for request in server.incoming_requests() {
        let current = active_threads.fetch_add(1, Ordering::AcqRel);
        if current >= MAX_THREADS {
            active_threads.fetch_sub(1, Ordering::Release);
            let resp = Response::from_string("Server overloaded")
                .with_status_code(StatusCode(503));
            let _ = request.respond(resp);
            continue;
        }

        let ctx = Arc::clone(&ctx);
        let active_threads = Arc::clone(&active_threads);

        std::thread::spawn(move || {
            handle(request, &ctx);
            active_threads.fetch_sub(1, Ordering::Release);
        });
    }
}

#[allow(clippy::too_many_lines)]
fn handle(mut req: Request, ctx: &Ctx) {
    let url = req.url().to_string();
    let method = req.method().clone();
    let client_ip = req.remote_addr().map_or_else(
        || IpAddr::from([127, 0, 0, 1]),
        std::net::SocketAddr::ip,
    );

    if url == "/api/health" && method == Method::Get {
        json_respond(req, &serde_json::json!({"status":"ok"}));
        return;
    }

    if url == "/api/check" && method == Method::Get {
        let mode_guard = ctx.mode.lock().unwrap_or_else(PoisonError::into_inner);
        let setup_needed = matches!(*mode_guard, AppMode::Setup);
        drop(mode_guard);
        let resp = serde_json::json!({ "setup_needed": setup_needed });
        json_respond(req, &resp);
        return;
    }

    if url == "/api/setup" && method == Method::Post {
        let body = match read_body_limited(&mut req, MAX_BODY_SIZE) {
            Ok(b) => b,
            Err(e) => { json_error(req, 400, &e); return; }
        };
        let mut mode_guard = ctx.mode.lock().unwrap_or_else(PoisonError::into_inner);
        match auth::handle_setup_body(&body, &ctx.config_path, &mut mode_guard) {
            Ok(username) => {
                let mut store = ctx.session_store.lock().unwrap_or_else(PoisonError::into_inner);
                let token = store.create(username);
                let duration = store.duration();
                drop(store);
                drop(mode_guard);
                let cookie = format!(
                    "simd_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={duration}"
                );
                let cookie_header: Header =
                    format!("Set-Cookie: {cookie}").parse().unwrap();
                let header: Header = "Content-Type: application/json".parse().unwrap();
                let resp = Response::from_string(r#"{"ok":true}"#)
                    .with_header(header)
                    .with_header(cookie_header);
                let _ = req.respond(resp);
            }
            Err(e) => {
                json_error(req, 400, &e);
            }
        }
        return;
    }

    {
        let mode_guard = ctx.mode.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*mode_guard, AppMode::Setup) {
            drop(mode_guard);
            if url == "/login" {
                let resp = Response::from_string("Redirecting...")
                    .with_status_code(StatusCode(302))
                    .with_header("Location: /".parse::<Header>().unwrap());
                let _ = req.respond(resp);
                return;
            }
            html_respond(req, SETUP_PAGE);
            return;
        }
    }

    if url == "/login" {
        html_respond(req, LOGIN_PAGE);
        return;
    }

    if url == "/api/login" && method == Method::Post {
        let body = match read_body_limited(&mut req, MAX_BODY_SIZE) {
            Ok(b) => b,
            Err(e) => { json_error(req, 400, &e); return; }
        };
        let mode_guard = ctx.mode.lock().unwrap_or_else(PoisonError::into_inner);
        let config = match &*mode_guard {
            AppMode::Normal(c) => c,
            AppMode::Setup => {
                drop(mode_guard);
                json_error(req, 400, "System not set up");
                return;
            }
        };
        match auth::handle_login(&body, config, &ctx.session_store, &ctx.rate_limiter, client_ip) {
            Ok(token) => {
                let duration = ctx.session_store.lock().unwrap_or_else(PoisonError::into_inner).duration();
                drop(mode_guard);
                let cookie = format!(
                    "simd_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={duration}"
                );
                let cookie_header: Header =
                    format!("Set-Cookie: {cookie}").parse().unwrap();
                let header: Header = "Content-Type: application/json".parse().unwrap();
                let resp = Response::from_string(r#"{"ok":true}"#)
                    .with_header(header)
                    .with_header(cookie_header);
                let _ = req.respond(resp);
            }
            Err(e) => {
                drop(mode_guard);
                json_error(req, 401, &e);
            }
        }
        return;
    }

    if url == "/api/logout" && method == Method::Post {
        if let Some(token) = auth::get_cookie(&req, "simd_session") {
            ctx.session_store.lock().unwrap_or_else(PoisonError::into_inner).remove(&token);
        }
        let cookie_header: Header =
            "Set-Cookie: simd_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"
                .parse()
                .unwrap();
        let header: Header = "Content-Type: application/json".parse().unwrap();
        let resp = Response::from_string(r#"{"ok":true}"#)
            .with_header(header)
            .with_header(cookie_header);
        let _ = req.respond(resp);
        return;
    }

    {
        let mode_guard = ctx.mode.lock().unwrap_or_else(PoisonError::into_inner);
        let config = match &*mode_guard {
            AppMode::Normal(c) => c,
            AppMode::Setup => {
                drop(mode_guard);
                json_error(req, 500, "Internal error");
                return;
            }
        };
        if !auth::is_authenticated(config, &req, &ctx.session_store) {
            drop(mode_guard);
            if url == "/" {
                let resp = Response::from_string("Redirecting...")
                    .with_status_code(StatusCode(302))
                    .with_header("Location: /login".parse::<Header>().unwrap());
                let _ = req.respond(resp);
                return;
            }
            if url.starts_with("/api/") {
                json_error(req, 401, "Authentication required");
                return;
            }
            let resp = Response::from_string("Redirecting...")
                .with_status_code(StatusCode(302))
                .with_header("Location: /login".parse::<Header>().unwrap());
            let _ = req.respond(resp);
            return;
        }
    }

    match url.as_str() {
        "/" => html_respond(req, APP_PAGE),
        u if u.starts_with("/api/tree") => tree::handle_tree_request(req, &ctx.data_path, &ctx.tree_cache),
        u if u.starts_with("/api/file") => {
            markdown::handle_file_request(req, u, &ctx.data_path, &ctx.file_cache);
        }
        u if u.starts_with("/api/cache-dir") => {
            markdown::handle_cache_dir(req, u, &ctx.data_path, &ctx.file_cache);
        }
        _ => {
            let resp = Response::from_string("Not Found".to_string())
                .with_status_code(StatusCode(404));
            let _ = req.respond(resp);
        }
    }
}

fn read_body_limited(req: &mut Request, max: usize) -> Result<String, String> {
    let mut body = String::new();
    req.as_reader()
        .take(max as u64)
        .read_to_string(&mut body)
        .map_err(|_| "Failed to read request body".to_string())?;
    if body.len() >= max {
        return Err("Request body too large".to_string());
    }
    Ok(body)
}

fn html_respond(req: Request, html: &str) {
    let ct: Header = "Content-Type: text/html; charset=utf-8".parse().unwrap();
    let cc: Header = "Cache-Control: no-cache".parse().unwrap();
    let nosniff: Header = "X-Content-Type-Options: nosniff".parse().unwrap();
    let xfo: Header = "X-Frame-Options: DENY".parse().unwrap();
    let resp = Response::from_string(html.to_string())
        .with_header(ct)
        .with_header(cc)
        .with_header(nosniff)
        .with_header(xfo);
    let _ = req.respond(resp);
}

fn json_respond<T: Serialize>(req: Request, data: &T) {
    let body = serde_json::to_string(data).unwrap();
    let header: Header = "Content-Type: application/json".parse().unwrap();
    let nosniff: Header = "X-Content-Type-Options: nosniff".parse().unwrap();
    let resp = Response::from_string(body)
        .with_header(header)
        .with_header(nosniff);
    let _ = req.respond(resp);
}

fn json_error(req: Request, status: u16, msg: &str) {
    let resp = serde_json::json!({ "error": msg });
    let body = serde_json::to_string(&resp).unwrap();
    let header: Header = "Content-Type: application/json".parse().unwrap();
    let nosniff: Header = "X-Content-Type-Options: nosniff".parse().unwrap();
    let response = Response::from_string(body)
        .with_header(header)
        .with_header(nosniff)
        .with_status_code(StatusCode(status));
    let _ = req.respond(response);
}

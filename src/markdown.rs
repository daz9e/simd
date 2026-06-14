use crate::cache::{FileCache, FileEntry};
use pulldown_cmark::{Parser, Options, html};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use tiny_http::{Header, Request, Response, StatusCode};

#[derive(Serialize)]
struct FileResponse {
    filename: String,
    raw: String,
    html: String,
}

pub fn handle_file_request(
    req: Request,
    url: &str,
    data_path: &Path,
    file_cache: &Mutex<FileCache>,
) {
    let Some(file_path) = extract_query_param(url, "path") else {
        let resp = Response::from_string("Missing 'path' parameter".to_string())
            .with_status_code(StatusCode(400));
        let _ = req.respond(resp);
        return;
    };

    let full_path = data_path.join(&file_path);

    let Ok(data_canonical) = data_path.canonicalize() else {
        let resp = Response::from_string("Data directory not accessible".to_string())
            .with_status_code(StatusCode(500));
        let _ = req.respond(resp);
        return;
    };

    let Ok(content) = fs::read_to_string(&full_path) else {
        let resp = Response::from_string("File not found".to_string())
            .with_status_code(StatusCode(404));
        let _ = req.respond(resp);
        return;
    };

    let Ok(canonical) = full_path.canonicalize() else {
        let resp = Response::from_string("File not found".to_string())
            .with_status_code(StatusCode(404));
        let _ = req.respond(resp);
        return;
    };

    if !canonical.starts_with(&data_canonical) {
        let resp = Response::from_string("Access denied".to_string())
            .with_status_code(StatusCode(403));
        let _ = req.respond(resp);
        return;
    }

    let Ok(mtime) = fs::metadata(&canonical).and_then(|m| m.modified()) else {
        let resp = Response::from_string("File not found".to_string())
            .with_status_code(StatusCode(404));
        let _ = req.respond(resp);
        return;
    };

    {
        let cache = file_cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = cache.get(&canonical, &mtime) {
            let response_data = FileResponse {
                filename: entry.filename.clone(),
                raw: entry.raw.clone(),
                html: entry.html.clone(),
            };
            let resp_body = serde_json::to_string(&response_data).unwrap();
            let header: Header = "Content-Type: application/json".parse().unwrap();
            let response = Response::from_string(resp_body).with_header(header);
            let _ = req.respond(response);
            return;
        }
    }

    let filename = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let html = render_markdown(&content);

    {
        let mut cache = file_cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.insert(canonical, FileEntry {
            mtime,
            filename: filename.clone(),
            raw: content.clone(),
            html: html.clone(),
        });
    }

    let response_data = FileResponse {
        filename,
        raw: content,
        html,
    };

    let resp_body = serde_json::to_string(&response_data).unwrap();
    let header: Header = "Content-Type: application/json".parse().unwrap();
    let response = Response::from_string(resp_body).with_header(header);
    let _ = req.respond(response);
}

fn render_markdown(content: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

fn extract_query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next().unwrap_or("");
        if key == name && !value.is_empty() {
            return Some(url_decode(value));
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(hex_val).unwrap_or(0);
            let lo = chars.next().and_then(hex_val).unwrap_or(0);
            result.push((hi << 4 | lo) as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn handle_cache_dir(
    req: Request,
    url: &str,
    data_path: &Path,
    file_cache: &Mutex<FileCache>,
) {
    let Some(dir_path) = extract_query_param(url, "path") else {
        let resp = Response::from_string("Missing 'path' parameter".to_string())
            .with_status_code(StatusCode(400));
        let _ = req.respond(resp);
        return;
    };

    let full_path = data_path.join(&dir_path);

    let Ok(data_canonical) = data_path.canonicalize() else {
        let resp = Response::from_string("Data directory not accessible".to_string())
            .with_status_code(StatusCode(500));
        let _ = req.respond(resp);
        return;
    };

    let Ok(canonical) = full_path.canonicalize() else {
        let resp = Response::from_string("Directory not found".to_string())
            .with_status_code(StatusCode(404));
        let _ = req.respond(resp);
        return;
    };

    if !canonical.starts_with(&data_canonical) {
        let resp = Response::from_string("Access denied".to_string())
            .with_status_code(StatusCode(403));
        let _ = req.respond(resp);
        return;
    }

    if !canonical.is_dir() {
        let resp = Response::from_string("Not a directory".to_string())
            .with_status_code(StatusCode(400));
        let _ = req.respond(resp);
        return;
    }

    let mut cached = 0u32;
    let mut skipped = 0u32;

    if let Ok(entries) = fs::read_dir(&canonical) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            let Ok(mtime) = fs::metadata(&path).and_then(|m| m.modified()) else { continue };

            {
                let cache = file_cache.lock().unwrap_or_else(PoisonError::into_inner);
                if cache.get(&path, &mtime).is_some() {
                    skipped += 1;
                    continue;
                }
            }

            let Ok(content) = fs::read_to_string(&path) else { continue };

            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let html = render_markdown(&content);

            let mut cache = file_cache.lock().unwrap_or_else(PoisonError::into_inner);
            cache.insert(path, FileEntry {
                mtime,
                filename,
                raw: content,
                html,
            });
            drop(cache);
            cached += 1;
        }
    }

    let resp = serde_json::json!({ "cached": cached, "skipped": skipped });
    json_respond_short(req, &resp);
}

fn json_respond_short<T: Serialize>(req: Request, data: &T) {
    let body = serde_json::to_string(data).unwrap();
    let header: Header = "Content-Type: application/json".parse().unwrap();
    let resp = Response::from_string(body).with_header(header);
    let _ = req.respond(resp);
}
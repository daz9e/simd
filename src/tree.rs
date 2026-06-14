use crate::cache::TreeCache;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use tiny_http::{Header, Request, Response};

#[derive(Serialize)]
pub struct TreeNode {
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Self>>,
    pub path: String,
}

#[derive(Serialize)]
struct TreeResponse {
    entries: Vec<TreeNode>,
}

pub fn handle_tree_request(req: Request, data_path: &Path, tree_cache: &Mutex<TreeCache>) {
    {
        let cache = tree_cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cached) = cache.get() {
            let resp_body = cached.to_string();
            let header: Header = "Content-Type: application/json".parse().unwrap();
            let response = Response::from_string(resp_body).with_header(header);
            let _ = req.respond(response);
            return;
        }
    }

    let tree = build_tree_impl(data_path, data_path);
    let resp_body = serde_json::to_string(&TreeResponse { entries: tree }).unwrap();

    {
        let mut cache = tree_cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.set(resp_body.clone());
    }

    let header: Header = "Content-Type: application/json".parse().unwrap();
    let response = Response::from_string(resp_body).with_header(header);
    let _ = req.respond(response);
}

fn build_tree_impl(root: &Path, base_path: &Path) -> Vec<TreeNode> {
    let mut entries = Vec::new();
    let Ok(dir) = fs::read_dir(base_path) else { return entries };

    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        if path.is_dir() {
            let children = build_tree_impl(root, &path);
            entries.push(TreeNode {
                name,
                kind: "dir".to_string(),
                children: Some(children),
                path: rel_path,
            });
        } else {
            entries.push(TreeNode {
                name,
                kind: "file".to_string(),
                children: None,
                path: rel_path,
            });
        }
    }

    entries.sort_by(|a, b| {
        let a_is_dir = a.kind == "dir";
        let b_is_dir = b.kind == "dir";
        if a_is_dir == b_is_dir {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            b_is_dir.cmp(&a_is_dir)
        }
    });

    entries
}
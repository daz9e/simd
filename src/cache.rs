use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub struct FileEntry {
    pub mtime: SystemTime,
    pub filename: String,
    pub raw: String,
    pub html: String,
}

pub struct FileCache {
    entries: HashMap<PathBuf, FileEntry>,
    order: Vec<PathBuf>,
    max: usize,
}

impl FileCache {
    pub fn new(max: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::with_capacity(max),
            max,
        }
    }

    pub fn get(&self, path: &PathBuf, mtime: &SystemTime) -> Option<&FileEntry> {
        let entry = self.entries.get(path)?;
        if entry.mtime == *mtime { Some(entry) } else { None }
    }

    pub fn insert(&mut self, path: PathBuf, entry: FileEntry) {
        if !self.entries.contains_key(&path) && self.entries.len() >= self.max {
            if let Some(old) = self.order.first().cloned() {
                self.entries.remove(&old);
                self.order.remove(0);
            }
        }
        self.entries.insert(path.clone(), entry);
        self.order.retain(|p| p != &path);
        self.order.push(path);
    }
}

pub struct TreeCache {
    pub data: Option<String>,
    pub last_updated: Instant,
    pub ttl: Duration,
}

impl TreeCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            data: None,
            last_updated: Instant::now(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn get(&self) -> Option<&str> {
        if self.last_updated.elapsed() < self.ttl {
            self.data.as_deref()
        } else {
            None
        }
    }

    pub fn set(&mut self, data: String) {
        self.data = Some(data);
        self.last_updated = Instant::now();
    }
}
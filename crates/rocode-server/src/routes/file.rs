use axum::{extract::Query, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use crate::{ApiError, Result, ServerState};

pub(crate) fn file_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_files))
        .route("/content", get(read_file))
        .route("/status", get(get_file_status))
}

pub(crate) fn find_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/text", get(find_text))
        .route("/file", get(find_files))
        .route("/symbol", get(find_symbols))
}

#[derive(Debug, Deserialize)]
pub struct ListFilesQuery {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: String,
    pub size: Option<u64>,
    pub modified: Option<i64>,
}

fn project_root() -> Result<PathBuf> {
    std::env::current_dir()
        .map_err(|e| ApiError::BadRequest(format!("Failed to resolve current directory: {}", e)))
}

fn canonicalize_within_root(path: &FsPath, root: &FsPath) -> Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .map_err(|e| ApiError::BadRequest(format!("Failed to resolve project root: {}", e)))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|e| ApiError::BadRequest(format!("Failed to resolve path: {}", e)))?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(ApiError::BadRequest(
            "Access denied: path escapes project directory".to_string(),
        ));
    }

    Ok(canonical_path)
}

fn resolve_input_path(input: &str, root: &FsPath) -> Result<PathBuf> {
    let path = PathBuf::from(input);
    let resolved = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    if !resolved.exists() {
        return Err(ApiError::NotFound("File not found".to_string()));
    }
    canonicalize_within_root(&resolved, root)
}

fn is_within_root(path: &FsPath, root: &FsPath) -> bool {
    canonicalize_within_root(path, root).is_ok()
}

async fn list_files(Query(query): Query<ListFilesQuery>) -> Result<Json<Vec<FileInfo>>> {
    let root = project_root()?;
    let path = resolve_input_path(&query.path, &root)?;
    let mut files = Vec::new();

    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.flatten() {
                let path_buf = entry.path();
                if !is_within_root(&path_buf, &root) {
                    continue;
                }
                let file_type = if path_buf.is_dir() {
                    "directory"
                } else {
                    "file"
                };
                let size = if path_buf.is_file() {
                    std::fs::metadata(&path_buf).ok().map(|m| m.len())
                } else {
                    None
                };
                let modified = std::fs::metadata(&path_buf).ok().and_then(|m| {
                    m.modified().ok().map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    })
                });

                files.push(FileInfo {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: path_buf.to_string_lossy().to_string(),
                    file_type: file_type.to_string(),
                    size,
                    modified,
                });
            }
        }
    }

    Ok(Json(files))
}

async fn read_file(Query(query): Query<ListFilesQuery>) -> Result<Json<serde_json::Value>> {
    let root = project_root()?;
    let path = resolve_input_path(&query.path, &root)?;

    if path.is_file() {
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(Json(
                serde_json::json!({ "content": content, "path": query.path }),
            )),
            Err(e) => Err(ApiError::BadRequest(format!("Failed to read file: {}", e))),
        }
    } else {
        Err(ApiError::BadRequest("Path is not a file".to_string()))
    }
}

async fn get_file_status() -> Result<Json<Vec<FileStatusInfo>>> {
    let cwd = std::env::current_dir()
        .map_err(|e| ApiError::BadRequest(format!("Failed to resolve current directory: {}", e)))?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .arg("status")
        .arg("--porcelain")
        .output()
        .map_err(|e| ApiError::BadRequest(format!("Failed to run git status: {}", e)))?;

    if !output.status.success() {
        return Ok(Json(Vec::new()));
    }

    let mut files = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.len() < 4 {
            continue;
        }
        let status_code = &line[..2];
        let mut path = line[3..].trim().to_string();
        if let Some((_, renamed_to)) = path.rsplit_once(" -> ") {
            path = renamed_to.to_string();
        }

        let staged = status_code.chars().next().unwrap_or(' ') != ' ';
        let status_char = if staged {
            status_code.chars().next().unwrap_or(' ')
        } else {
            status_code.chars().nth(1).unwrap_or(' ')
        };
        let status = match status_char {
            'M' => "modified",
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            'C' => "copied",
            'U' => "unmerged",
            '?' => "untracked",
            _ => "unknown",
        };

        files.push(FileStatusInfo {
            path,
            status: status.to_string(),
            staged,
        });
    }

    Ok(Json(files))
}

#[derive(Debug, Serialize)]
pub struct FileStatusInfo {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[derive(Debug, Deserialize)]
pub struct FindTextQuery {
    pub pattern: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub match_text: String,
}

async fn find_text(Query(query): Query<FindTextQuery>) -> Result<Json<Vec<SearchResult>>> {
    let root = project_root()?;
    let base_input = query
        .path
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let base_path = resolve_input_path(&base_input, &root)?;
    let mut results = Vec::new();

    fn search_in_file(path: &std::path::Path, pattern: &str, results: &mut Vec<SearchResult>) {
        if let Ok(content) = std::fs::read_to_string(path) {
            for (line_num, line) in content.lines().enumerate() {
                if let Some(col) = line.find(pattern) {
                    results.push(SearchResult {
                        path: path.to_string_lossy().to_string(),
                        line: line_num + 1,
                        column: col + 1,
                        match_text: line.to_string(),
                    });
                }
            }
        }
    }

    fn search_recursive(
        path: &FsPath,
        root: &FsPath,
        pattern: &str,
        results: &mut Vec<SearchResult>,
    ) {
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let path_buf = entry.path();
                    if !is_within_root(&path_buf, root) {
                        continue;
                    }
                    if path_buf.is_dir() {
                        search_recursive(&path_buf, root, pattern, results);
                    } else if path_buf.is_file() {
                        search_in_file(&path_buf, pattern, results);
                    }
                }
            }
        }
    }

    search_recursive(&base_path, &root, &query.pattern, &mut results);
    Ok(Json(results))
}

#[derive(Debug, Deserialize)]
pub struct FindFilesQuery {
    pub query: String,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub limit: Option<usize>,
}

async fn find_files(Query(query): Query<FindFilesQuery>) -> Result<Json<Vec<String>>> {
    let base_path = project_root()?;
    let mut results = Vec::new();
    let limit = query.limit.unwrap_or(100);

    fn find_recursive(
        path: &FsPath,
        root: &FsPath,
        query: &str,
        results: &mut Vec<String>,
        limit: usize,
    ) {
        if results.len() >= limit {
            return;
        }
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let path_buf = entry.path();
                    if !is_within_root(&path_buf, root) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(query) {
                        results.push(path_buf.to_string_lossy().to_string());
                    }
                    if path_buf.is_dir() && results.len() < limit {
                        find_recursive(&path_buf, root, query, results, limit);
                    }
                }
            }
        }
    }

    find_recursive(&base_path, &base_path, &query.query, &mut results, limit);
    Ok(Json(results))
}

#[derive(Debug, Deserialize)]
pub struct FindSymbolsQuery {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
}

async fn find_symbols(Query(_query): Query<FindSymbolsQuery>) -> Result<Json<Vec<SymbolInfo>>> {
    Ok(Json(Vec::new()))
}

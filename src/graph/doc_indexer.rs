use std::path::Path;
use std::time::SystemTime;

use rusqlite::Connection;

use crate::db::queries;

const MAX_CHUNK_LINES: usize = 100;

#[derive(Debug)]
pub struct DocChunk {
    pub heading: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub token_estimate: u32,
    pub content_hash: String,
}

/// Split markdown content at heading boundaries, sub-split at MAX_CHUNK_LINES.
pub fn chunk_markdown(content: &str) -> Vec<DocChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_start: usize = 0;
    let mut current_lines: Vec<&str> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with('#') {
            if !current_lines.is_empty() {
                emit_chunks(
                    &current_heading,
                    current_start,
                    &current_lines,
                    &mut chunks,
                );
            }
            current_heading = Some(line.trim_start_matches('#').trim().to_string());
            current_start = i;
            current_lines = vec![line];
        } else {
            current_lines.push(line);
        }
    }

    if !current_lines.is_empty() {
        emit_chunks(
            &current_heading,
            current_start,
            &current_lines,
            &mut chunks,
        );
    }

    chunks
}

fn emit_chunks(
    heading: &Option<String>,
    start_offset: usize,
    lines: &[&str],
    out: &mut Vec<DocChunk>,
) {
    for sub_chunk in lines.chunks(MAX_CHUNK_LINES) {
        let chunk_start_idx = out.len();
        let content = sub_chunk.join("\n");
        let token_estimate = (content.len() / 4) as u32;
        let hash = format!("{:x}", md5::compute(content.as_bytes()));
        let content_hash = hash[..8].to_string();

        let start_line = (start_offset + chunk_start_idx * MAX_CHUNK_LINES) as u32 + 1;
        let end_line = start_line + sub_chunk.len() as u32 - 1;

        out.push(DocChunk {
            heading: heading.clone(),
            start_line,
            end_line,
            content,
            token_estimate,
            content_hash,
        });
    }
}

/// Index a single markdown file into the doc_chunks table.
pub fn index_doc_file(
    conn: &Connection,
    file_path: &str,
    content: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let chunks = chunk_markdown(content);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    queries::delete_doc_chunks_for_file(conn, file_path)?;

    let raw_token_estimate = (content.len() / 4) as u32;
    queries::upsert_file(conn, file_path, "doc", raw_token_estimate, now)?;

    for (i, chunk) in chunks.iter().enumerate() {
        queries::upsert_doc_chunk(
            conn,
            file_path,
            i as u32,
            chunk.heading.as_deref(),
            chunk.start_line,
            chunk.end_line,
            &chunk.content,
            chunk.token_estimate,
            now,
            &chunk.content_hash,
        )?;
    }

    Ok(chunks.len() as u32)
}

/// Collect all doc files under a root directory using the `ignore` crate.
pub fn collect_doc_files(root: &Path, _patterns: &[String], exclude: &[String]) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "md" || ext == "markdown" {
                let path_str = path.to_string_lossy();
                let excluded = exclude.iter().any(|pat| {
                    let pat_simple = pat.trim_start_matches("**/").trim_end_matches("/**");
                    path_str.contains(pat_simple)
                });
                if !excluded {
                    files.push(entry.into_path());
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_markdown_basic() {
        let content = "# Title\nSome text\n## Section\nMore text";
        let chunks = chunk_markdown(content);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading.as_deref(), Some("Title"));
        assert_eq!(chunks[1].heading.as_deref(), Some("Section"));
    }

    #[test]
    fn test_chunk_markdown_no_headings() {
        let content = "Just some text\nwith no headings";
        let chunks = chunk_markdown(content);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_none());
    }

    #[test]
    fn test_chunk_markdown_long_section() {
        let lines: Vec<String> = (0..150).map(|i| format!("Line {i}")).collect();
        let content = format!("# Long\n{}", lines.join("\n"));
        let chunks = chunk_markdown(&content);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn test_index_doc_file() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        let content = "# Hello\nWorld\n## Section\nContent here";
        let count = index_doc_file(&conn, "README.md", content).unwrap();
        assert_eq!(count, 2);
    }
}

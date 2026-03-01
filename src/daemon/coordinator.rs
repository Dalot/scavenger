use std::sync::Arc;

use crate::db;
use crate::db::queries;
use crate::graph::index;
use crate::memory;

use super::{detect_branch, DaemonState, ReindexState};

/// ReindexCoordinator: handles branch detection, DB open/close, and freshness scanning.
pub struct ReindexCoordinator;

impl ReindexCoordinator {
    /// Warm switch: branch already has an index DB.
    pub fn check_branch_switch(state: &Arc<DaemonState>) -> Result<bool, Box<dyn std::error::Error>> {
        let new_branch = detect_branch(&state.project_root);
        let current = state.current_branch.read().clone();

        if new_branch == current {
            return Ok(false);
        }

        eprintln!("Branch switch detected: {current} → {new_branch}");
        *state.reindex_state.write() = ReindexState::Switching;

        // Close current branch DB
        {
            let mut db_guard = state.branch_db.lock();
            *db_guard = None;
        }

        // Check if new branch has an existing DB (warm) or needs cold start
        let indexes_dir = state.scavenger_dir.join("indexes");
        let sanitized = new_branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let new_db_path = indexes_dir.join(format!("{sanitized}.db"));
        let is_cold = !new_db_path.exists();

        if is_cold {
            Self::cold_start(state, &new_branch, &current)?;
        } else {
            // Warm switch: open existing DB, reload graph
            let new_conn = db::open_branch_db(&state.scavenger_dir, &new_branch)?;

            {
                let mut g = state.graph.write();
                g.load_from_db(&new_conn)?;
                g.compute_pagerank(0.85, 30);
            }

            {
                let mut db_guard = state.branch_db.lock();
                *db_guard = Some(new_conn);
            }
        }

        *state.current_branch.write() = new_branch.clone();

        // Update daemon_meta
        {
            let meta = state.meta_db.lock();
            queries::set_meta(&meta, "current_branch", &new_branch)?;
        }

        // Check for merge commit
        Self::check_merge_commit(state, &new_branch)?;

        *state.reindex_state.write() = ReindexState::Ready;
        eprintln!("Branch switch complete: now on {new_branch}");
        Ok(true)
    }

    /// Cold start: new branch with no existing DB.
    /// Copy parent branch DB, clear ephemeral data, re-index changed files.
    fn cold_start(
        state: &Arc<DaemonState>,
        new_branch: &str,
        parent_branch: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Cold start for branch {new_branch} (parent: {parent_branch})");
        *state.reindex_state.write() = ReindexState::ColdStart;

        // Open parent DB to fork annotations
        let parent_conn = db::open_branch_db(&state.scavenger_dir, parent_branch)?;

        // Create new branch DB
        let new_conn = db::open_branch_db(&state.scavenger_dir, new_branch)?;

        // Fork annotations from parent
        let forked = memory::MemoryManager::fork_annotations(&parent_conn, &new_conn)?;
        eprintln!("Cold start: forked {forked} annotations from {parent_branch}");

        // Get changed files via git diff
        let changed_files = git_diff_files(&state.project_root, parent_branch);

        if !changed_files.is_empty() {
            let mut g = state.graph.write();
            g.load_from_db(&new_conn)?;
            let _ = index::bulk_index(&new_conn, &mut g, &changed_files)?;
            g.compute_pagerank(0.85, 30);
            let _ = g.save_centrality(&new_conn);
        } else {
            let mut g = state.graph.write();
            g.load_from_db(&new_conn)?;
            g.compute_pagerank(0.85, 30);
        }

        {
            let mut db_guard = state.branch_db.lock();
            *db_guard = Some(new_conn);
        }

        Ok(())
    }

    /// After branch switch, check if HEAD is a merge commit. If so,
    /// trigger annotation union-merge from source branches.
    fn check_merge_commit(
        state: &Arc<DaemonState>,
        _branch: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%P", "HEAD"])
            .current_dir(&state.project_root)
            .output()?;

        let parents = String::from_utf8_lossy(&output.stdout);
        let parent_hashes: Vec<&str> = parents.trim().split_whitespace().collect();

        if parent_hashes.len() < 2 {
            return Ok(());
        }

        eprintln!("Merge commit detected ({} parents), triggering annotation merge", parent_hashes.len());

        // Find source branches from reflog
        let source_branches = find_merged_branches(&state.project_root);
        for source_branch in &source_branches {
            let indexes_dir = state.scavenger_dir.join("indexes");
            let sanitized = source_branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
            let source_db_path = indexes_dir.join(format!("{sanitized}.db"));

            if source_db_path.exists() {
                if let Ok(source_conn) = db::open_branch_db(&state.scavenger_dir, source_branch) {
                    let db_guard = state.branch_db.lock();
                    if let Some(ref target_conn) = *db_guard {
                        match memory::MemoryManager::merge_annotations(&source_conn, target_conn) {
                            Ok(result) => {
                                eprintln!(
                                    "Merged annotations from {source_branch}: {} imported, {} deduped",
                                    result.imported, result.deduped
                                );
                            }
                            Err(e) => eprintln!("Annotation merge error from {source_branch}: {e}"),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Perform a freshness scan: compare filesystem state against indexed files.
    #[allow(dead_code)]
    pub fn freshness_scan(state: &Arc<DaemonState>) -> Result<FreshnessResult, Box<dyn std::error::Error>> {
        *state.reindex_state.write() = ReindexState::Indexing;

        let source_files = index::collect_source_files(&state.project_root);
        let mut stale_files = Vec::new();

        {
            let db_guard = state.branch_db.lock();
            let Some(ref conn) = *db_guard else {
                *state.reindex_state.write() = ReindexState::Ready;
                return Ok(FreshnessResult { stale_count: 0, reindexed: 0 });
            };

            for path in &source_files {
                let file_path_str = path.to_string_lossy().to_string();
                let last_indexed = queries::get_file_last_indexed(conn, &file_path_str)?;

                let fs_mtime = std::fs::metadata(path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                match last_indexed {
                    Some(indexed_at) if indexed_at >= fs_mtime => {}
                    _ => stale_files.push(path.clone()),
                }
            }
        }

        let stale_count = stale_files.len() as u64;

        if !stale_files.is_empty() {
            let db_guard = state.branch_db.lock();
            if let Some(ref conn) = *db_guard {
                let mut g = state.graph.write();
                let _stats = index::bulk_index(conn, &mut g, &stale_files)?;
            }
        }

        *state.reindex_state.write() = ReindexState::Ready;

        Ok(FreshnessResult {
            stale_count,
            reindexed: stale_count,
        })
    }

    /// Branch cleanup: delete DB files for branches that no longer exist.
    /// Compares on-disk index files against `git branch` output.
    #[allow(dead_code)]
    pub fn cleanup_deleted_branches(state: &Arc<DaemonState>) -> Result<u64, Box<dyn std::error::Error>> {
        let live_branches = list_git_branches(&state.project_root);
        let live_sanitized: std::collections::HashSet<String> = live_branches
            .iter()
            .map(|b| b.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"))
            .collect();

        let indexes_dir = state.scavenger_dir.join("indexes");
        if !indexes_dir.exists() {
            return Ok(0);
        }

        let current = state.current_branch.read().clone();
        let current_sanitized = current.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");

        let mut cleaned = 0u64;
        for entry in std::fs::read_dir(&indexes_dir)?.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(branch_name) = name.strip_suffix(".db") {
                    if branch_name == current_sanitized {
                        continue;
                    }
                    if !live_sanitized.contains(branch_name) {
                        let _ = std::fs::remove_file(entry.path());
                        eprintln!("Cleaned up stale branch DB: {name}");
                        cleaned += 1;
                    }
                }
            }
        }

        Ok(cleaned)
    }
}

/// Get files changed between current HEAD and a branch.
fn git_diff_files(project_root: &std::path::Path, base_branch: &str) -> Vec<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", base_branch, "HEAD"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| project_root.join(l.trim()))
                .filter(|p| p.exists())
                .collect()
        }
        _ => {
            // Fallback: index all source files
            index::collect_source_files(project_root)
        }
    }
}

/// Find source branches for the most recent merge commit.
/// Uses `git branch --points-at <second-parent-hash>` per design §8.5.
fn find_merged_branches(project_root: &std::path::Path) -> Vec<String> {
    // Get parent hashes of HEAD
    let parents_output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%P", "HEAD"])
        .current_dir(project_root)
        .output();

    let parent_hashes: Vec<String> = match parents_output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        }
        _ => return Vec::new(),
    };

    if parent_hashes.len() < 2 {
        return Vec::new();
    }

    // For each non-first parent, find which branches point at that commit
    let mut branches = Vec::new();
    for parent_hash in &parent_hashes[1..] {
        let output = std::process::Command::new("git")
            .args(["branch", "--points-at", parent_hash, "--format=%(refname:short)"])
            .current_dir(project_root)
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let branch = line.trim().to_string();
                    if !branch.is_empty() && !branches.contains(&branch) {
                        branches.push(branch);
                    }
                }
            }
        }
    }

    branches
}

/// List all local git branches.
#[allow(dead_code)]
fn list_git_branches(project_root: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct FreshnessResult {
    pub stale_count: u64,
    pub reindexed: u64,
}

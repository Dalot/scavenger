use super::{CandidateItem, CandidateSource, OutputGroup};

/// PIN stage: mark pinned items (target, behavioral signals, 1-hop structural).
/// 1-hop callers/callees are semi-pinned: guaranteed included, signatures only.
pub fn pin(candidates: &mut [CandidateItem]) {
    for item in candidates.iter_mut() {
        item.pinned = matches!(
            item.source,
            CandidateSource::Target
                | CandidateSource::BehavioralSignal
                | CandidateSource::Caller
                | CandidateSource::Callee
        );
    }
}

/// TRIM stage: sort unpinned by score DESC, greedy fill remaining budget.
/// Skip oversized items, continue to next. Returns items that fit within budget.
pub fn trim(candidates: &mut Vec<CandidateItem>, budget: u32) {
    let pinned_tokens: u32 = candidates
        .iter()
        .filter(|c| c.pinned)
        .map(|c| c.token_count)
        .sum();

    if pinned_tokens >= budget {
        candidates.retain(|c| c.pinned);
        return;
    }

    let remaining_budget = budget - pinned_tokens;

    let mut unpinned: Vec<_> = candidates.iter().filter(|c| !c.pinned).cloned().collect();

    unpinned.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut used = 0u32;
    let mut keep_unpinned = Vec::new();
    for item in unpinned {
        if used + item.token_count <= remaining_budget {
            used += item.token_count;
            keep_unpinned.push(item);
        }
    }

    let pinned: Vec<_> = candidates.iter().filter(|c| c.pinned).cloned().collect();
    candidates.clear();
    candidates.extend(pinned);
    candidates.extend(keep_unpinned);
}

/// GROUP stage: assign each candidate to an output group.
pub fn group(candidates: &mut [CandidateItem]) {
    for item in candidates.iter_mut() {
        item.group = Some(match item.source {
            CandidateSource::Target => OutputGroup::Target,
            CandidateSource::BehavioralSignal => OutputGroup::Signal,
            CandidateSource::Caller => OutputGroup::Callers,
            CandidateSource::Callee => OutputGroup::Callees,
            CandidateSource::Annotation
            | CandidateSource::NodeHistory
            | CandidateSource::SessionActivity => OutputGroup::Context,
            CandidateSource::DocChunk => OutputGroup::Documentation,
            CandidateSource::GraphNode => OutputGroup::Context,
        });
    }
}

/// Target body information for [BODY] section inclusion.
pub struct TargetBody {
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub name: String,
}

/// RENDER stage: emit the final capsule text following section ordering per FR-018.
/// Order: [!] → [TARGET] → [CALLERS] → [CALLEES] → [CONTEXT] → [DOCUMENTATION] → [BODY]
/// Empty sections are omitted. Scores are NOT in output.
/// If `target_body` is provided and `include_body` is true and remaining budget > 200 tokens, the full body is appended.
pub fn render(
    candidates: &[CandidateItem],
    remaining_budget: u32,
    target_body: Option<&TargetBody>,
    include_body: bool,
) -> String {
    let section_order = [
        OutputGroup::Signal,
        OutputGroup::Target,
        OutputGroup::Callers,
        OutputGroup::Callees,
        OutputGroup::Context,
        OutputGroup::Documentation,
    ];

    let mut sections = Vec::new();

    for group in &section_order {
        let items: Vec<&CandidateItem> = candidates
            .iter()
            .filter(|c| c.group.as_ref() == Some(group))
            .collect();

        if items.is_empty() {
            continue;
        }

        let header = group.header();
        let body: Vec<&str> = items.iter().map(|c| c.content.as_str()).collect();
        sections.push(format!("{}\n{}", header, body.join("\n")));
    }

    if include_body
        && remaining_budget > 200
        && let Some(tb) = target_body
        && let Ok(body_text) = read_body_from_file(&tb.file_path, tb.line_start, tb.line_end)
    {
        let body_tokens = (body_text.len() / 4) as u32;
        if body_tokens <= remaining_budget {
            sections.push(format!("[BODY] {}\n{}", tb.name, body_text));
        }
    }

    sections.join("\n\n")
}

fn read_body_from_file(
    file_path: &str,
    line_start: u32,
    line_end: u32,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = (line_start as usize).saturating_sub(1).min(lines.len());
    let end = (line_end as usize).min(lines.len());
    if start >= end {
        return Ok(String::new());
    }
    Ok(lines[start..end].join("\n"))
}

impl OutputGroup {
    pub fn header(&self) -> &'static str {
        match self {
            Self::Signal => "[!]",
            Self::Target => "[TARGET]",
            Self::Callers => "[CALLERS]",
            Self::Callees => "[CALLEES]",
            Self::Context => "[CONTEXT]",
            Self::Documentation => "[DOCUMENTATION]",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::NodeId;

    fn make_item(source: CandidateSource, content: &str, tokens: u32, score: f64) -> CandidateItem {
        CandidateItem {
            content: content.to_string(),
            token_count: tokens,
            source,
            node_id: Some(NodeId("t1".to_string())),
            file_path: None,
            stale: false,
            priority_doc: false,
            score,
            pinned: false,
            group: None,
            anchor_type: None,
            version_distance: None,
            change_significance: None,
            bm25_score: None,
            timestamp: None,
            annotation_kind: None,
            quality: None,
            annotation_id: None,
        }
    }

    #[test]
    fn test_pin_marks_target_signals_and_structural() {
        let mut items = vec![
            make_item(CandidateSource::Target, "fn target()", 3, 1.0),
            make_item(CandidateSource::BehavioralSignal, "[!] THRASHING", 4, 1.0),
            make_item(CandidateSource::Caller, "fn caller()", 3, 0.5),
            make_item(CandidateSource::GraphNode, "fn other()", 3, 0.3),
        ];
        pin(&mut items);
        assert!(items[0].pinned, "Target should be pinned");
        assert!(items[1].pinned, "Signal should be pinned");
        assert!(items[2].pinned, "Caller (semi-pinned) should be pinned");
        assert!(
            !items[3].pinned,
            "Non-structural GraphNode should not be pinned"
        );
    }

    #[test]
    fn test_trim_respects_budget() {
        let mut items = vec![
            make_item(CandidateSource::Target, "target", 100, 1.0),
            make_item(CandidateSource::Caller, "c1", 200, 0.8),
            make_item(CandidateSource::Caller, "c2", 200, 0.6),
            make_item(CandidateSource::Caller, "c3", 200, 0.4),
        ];
        items[0].pinned = true;
        trim(&mut items, 400);
        // Pinned (100) + one caller (200) = 300 fits; two callers (400) fits too
        let total_tokens: u32 = items.iter().map(|i| i.token_count).sum();
        assert!(total_tokens <= 400);
    }

    #[test]
    fn test_group_assignment() {
        let mut items = vec![
            make_item(CandidateSource::Target, "target", 3, 1.0),
            make_item(CandidateSource::Caller, "caller", 3, 0.5),
            make_item(CandidateSource::DocChunk, "doc", 3, 0.3),
        ];
        group(&mut items);
        assert_eq!(items[0].group, Some(OutputGroup::Target));
        assert_eq!(items[1].group, Some(OutputGroup::Callers));
        assert_eq!(items[2].group, Some(OutputGroup::Documentation));
    }

    fn full_item(
        source: CandidateSource,
        content: &str,
        score: f64,
        pinned: bool,
        group: OutputGroup,
    ) -> CandidateItem {
        CandidateItem {
            content: content.to_string(),
            token_count: (content.len() / 4).max(1) as u32,
            source,
            node_id: None,
            file_path: None,
            stale: false,
            priority_doc: false,
            score,
            pinned,
            group: Some(group),
            anchor_type: None,
            version_distance: None,
            change_significance: None,
            bm25_score: None,
            timestamp: None,
            annotation_kind: None,
            quality: None,
            annotation_id: None,
        }
    }

    #[test]
    fn test_render_section_ordering() {
        let items = vec![
            full_item(
                CandidateSource::Target,
                "fn target()",
                1.0,
                true,
                OutputGroup::Target,
            ),
            full_item(
                CandidateSource::BehavioralSignal,
                "[!] THRASHING: repeated edits",
                1.0,
                true,
                OutputGroup::Signal,
            ),
            full_item(
                CandidateSource::Caller,
                "fn caller()",
                0.5,
                false,
                OutputGroup::Callers,
            ),
        ];
        let output = render(&items, 1000, None, true);
        let signal_pos = output.find("[!]").unwrap();
        let target_pos = output.find("[TARGET]").unwrap();
        let callers_pos = output.find("[CALLERS]").unwrap();
        assert!(signal_pos < target_pos);
        assert!(target_pos < callers_pos);
    }

    #[test]
    fn test_render_omits_empty_sections() {
        let items = vec![full_item(
            CandidateSource::Target,
            "fn target()",
            1.0,
            true,
            OutputGroup::Target,
        )];
        let output = render(&items, 1000, None, true);
        assert!(output.contains("[TARGET]"));
        assert!(!output.contains("[CALLERS]"));
    }

    #[test]
    fn test_render_body_inclusion_above_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn hello() {\n    println!(\"hello\");\n}\n").unwrap();

        let items = vec![full_item(
            CandidateSource::Target,
            "fn hello()",
            1.0,
            true,
            OutputGroup::Target,
        )];

        let tb = TargetBody {
            file_path: file_path.to_string_lossy().to_string(),
            line_start: 1,
            line_end: 3,
            name: "hello".to_string(),
        };

        let output = render(&items, 500, Some(&tb), true);
        assert!(
            output.contains("[BODY] hello"),
            "should include [BODY] section: {output}"
        );
        assert!(
            output.contains("println!"),
            "body should include function content"
        );
    }

    #[test]
    fn test_render_body_not_included_below_threshold() {
        let items = vec![full_item(
            CandidateSource::Target,
            "fn hello()",
            1.0,
            true,
            OutputGroup::Target,
        )];

        let tb = TargetBody {
            file_path: "/nonexistent".to_string(),
            line_start: 1,
            line_end: 3,
            name: "hello".to_string(),
        };

        let output = render(&items, 100, Some(&tb), true);
        assert!(
            !output.contains("[BODY]"),
            "should not include [BODY] when budget <= 200"
        );
    }

    #[test]
    fn test_render_body_not_included_when_flag_false() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn hello() {\n    println!(\"hello\");\n}\n").unwrap();

        let items = vec![full_item(
            CandidateSource::Target,
            "fn hello()",
            1.0,
            true,
            OutputGroup::Target,
        )];

        let tb = TargetBody {
            file_path: file_path.to_string_lossy().to_string(),
            line_start: 1,
            line_end: 3,
            name: "hello".to_string(),
        };

        let output = render(&items, 500, Some(&tb), false);
        assert!(
            !output.contains("[BODY]"),
            "should not include [BODY] when include_body=false: {output}"
        );
    }
}

use super::{CandidateItem, CandidateSource, OutputGroup};

/// PIN stage: mark pinned items (target, behavioral signals, 1-hop structural).
pub fn pin(candidates: &mut [CandidateItem]) {
    for item in candidates.iter_mut() {
        item.pinned = matches!(
            item.source,
            CandidateSource::Target | CandidateSource::BehavioralSignal
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

    let mut unpinned: Vec<_> = candidates
        .iter()
        .filter(|c| !c.pinned)
        .cloned()
        .collect();

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
            CandidateSource::Annotation | CandidateSource::NodeHistory | CandidateSource::SessionActivity => {
                OutputGroup::Context
            }
            CandidateSource::DocChunk => OutputGroup::Documentation,
            CandidateSource::GraphNode => {
                OutputGroup::Context
            }
        });
    }
}

/// RENDER stage: emit the final capsule text following section ordering per FR-018.
/// Order: [!] → [TARGET] → [CALLERS] → [CALLEES] → [CONTEXT] → [DOCUMENTATION] → [BODY]
/// Empty sections are omitted. Scores are NOT in output.
pub fn render(candidates: &[CandidateItem], remaining_budget: u32) -> String {
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

    // [BODY] inclusion: if remaining budget > 200 tokens, could include body
    // For now, [BODY] is a placeholder for future full-body inclusion
    if remaining_budget > 200 {
        // Body inclusion deferred — would need the actual source body
    }

    sections.join("\n\n")
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
        }
    }

    #[test]
    fn test_pin_marks_target_and_signals() {
        let mut items = vec![
            make_item(CandidateSource::Target, "fn target()", 3, 1.0),
            make_item(CandidateSource::BehavioralSignal, "[!] THRASHING", 4, 1.0),
            make_item(CandidateSource::Caller, "fn caller()", 3, 0.5),
        ];
        pin(&mut items);
        assert!(items[0].pinned);
        assert!(items[1].pinned);
        assert!(!items[2].pinned);
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

    #[test]
    fn test_render_section_ordering() {
        let items = vec![
            CandidateItem {
                content: "fn target()".to_string(),
                token_count: 3,
                source: CandidateSource::Target,
                node_id: None,
                file_path: None,
                stale: false,
                priority_doc: false,
                score: 1.0,
                pinned: true,
                group: Some(OutputGroup::Target),
            },
            CandidateItem {
                content: "[!] THRASHING: repeated edits".to_string(),
                token_count: 5,
                source: CandidateSource::BehavioralSignal,
                node_id: None,
                file_path: None,
                stale: false,
                priority_doc: false,
                score: 1.0,
                pinned: true,
                group: Some(OutputGroup::Signal),
            },
            CandidateItem {
                content: "fn caller()".to_string(),
                token_count: 3,
                source: CandidateSource::Caller,
                node_id: None,
                file_path: None,
                stale: false,
                priority_doc: false,
                score: 0.5,
                pinned: false,
                group: Some(OutputGroup::Callers),
            },
        ];
        let output = render(&items, 1000);
        let signal_pos = output.find("[!]").unwrap();
        let target_pos = output.find("[TARGET]").unwrap();
        let callers_pos = output.find("[CALLERS]").unwrap();
        assert!(signal_pos < target_pos);
        assert!(target_pos < callers_pos);
    }

    #[test]
    fn test_render_omits_empty_sections() {
        let items = vec![CandidateItem {
            content: "fn target()".to_string(),
            token_count: 3,
            source: CandidateSource::Target,
            node_id: None,
            file_path: None,
            stale: false,
            priority_doc: false,
            score: 1.0,
            pinned: true,
            group: Some(OutputGroup::Target),
        }];
        let output = render(&items, 1000);
        assert!(output.contains("[TARGET]"));
        assert!(!output.contains("[CALLERS]"));
        assert!(!output.contains("[!]"));
    }
}

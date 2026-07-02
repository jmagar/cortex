use super::*;
use serde_json::json;

#[test]
fn extracts_top_level_attribution_skill_and_plugin() {
    let value = json!({
        "sessionId": "sess-1",
        "attributionSkill": "cortex-troubleshoot",
        "attributionPlugin": "cortex",
        "content": "ran the troubleshoot skill"
    });
    let events = extract_claude_skill_events(&value);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.skill_name, "cortex-troubleshoot");
    assert_eq!(event.skill_plugin.as_deref(), Some("cortex"));
    assert_eq!(event.event_kind, SkillEventKind::ClaudeAttribution);
    assert_eq!(event.evidence_kind, SkillEvidenceKind::StructuredJsonField);
}

#[test]
fn extracts_nested_message_attribution_fields() {
    let value = json!({
        "message": {
            "attributionSkill": "web-app-testing",
            "attributionPlugin": "testing",
            "content": "tested the app"
        }
    });
    let events = extract_claude_skill_events(&value);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].skill_name, "web-app-testing");
    assert_eq!(events[0].skill_plugin.as_deref(), Some("testing"));
}

#[test]
fn emits_nothing_when_attribution_fields_absent() {
    let value = json!({"sessionId": "sess-1", "content": "just chatting"});
    assert!(extract_claude_skill_events(&value).is_empty());
}

#[test]
fn emits_nothing_for_empty_or_whitespace_skill_name() {
    let value = json!({"attributionSkill": "   "});
    assert!(extract_claude_skill_events(&value).is_empty());
}

#[test]
fn does_not_fabricate_plugin_skill_combined_string() {
    // Claude gives plugin and skill as SEPARATE fields — the combined
    // "plugin:skill" form must only appear when the source field itself
    // already used that format (see codex tests for that case).
    let value = json!({
        "attributionSkill": "sonnar",
        "attributionPlugin": "arrs"
    });
    let events = extract_claude_skill_events(&value);
    assert_eq!(events[0].skill_name, "sonnar");
    assert_eq!(events[0].skill_plugin.as_deref(), Some("arrs"));
}

#[test]
fn rejects_skill_name_containing_control_characters() {
    // Eng review Fix 8: a crafted attributionSkill value embedding an ANSI
    // escape sequence must be rejected, not silently stored — the CLI
    // printer (Task 9) uses println! directly on skill_name, so control
    // characters would let a malicious transcript spoof terminal output.
    let value = json!({
        "attributionSkill": "\u{1b}[2J\u{1b}[31mFAKE",
    });
    assert!(extract_claude_skill_events(&value).is_empty());
}

#[test]
fn rejects_skill_name_containing_embedded_newline() {
    let value = json!({
        "attributionSkill": "cortex-troubleshoot\nFAKE APPROVED LINE",
    });
    assert!(extract_claude_skill_events(&value).is_empty());
}

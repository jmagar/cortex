use serde_json::{Value, json};

use super::actions;

struct AdminActionHelp {
    action: &'static str,
    description: &'static str,
    parameters: &'static [&'static str],
}

const ADMIN_ACTION_HELP: &[AdminActionHelp] = &[
    AdminActionHelp {
        action: "ack_error",
        description: "Acknowledge an error signature so it is suppressed from future `unaddressed_errors`\nresults. Writes an audit event and updates the acknowledgement projection. Use\n`unack_error` to revoke.",
        parameters: &[
            "`signature_hash` (string, **required**) - the SHA-256 hash from `unaddressed_errors`",
            "`notes` (string, optional) - acknowledgement notes (max 4096 chars)",
        ],
    },
    AdminActionHelp {
        action: "unack_error",
        description: "Revoke an existing acknowledgement on an error signature so it reappears in\n`unaddressed_errors`. Writes an unack audit event; does NOT delete the ack history.",
        parameters: &[
            "`signature_hash` (string, **required**) - the SHA-256 hash of the signature",
            "`reason` (string, optional) - reason for removing the acknowledgement (max 4096 chars)",
        ],
    },
    AdminActionHelp {
        action: "file_tails",
        description: "Manage Cortex-owned file-tail ingest sources. Sources are stored in the local file-tail registry and reconciled by the runtime supervisor.",
        parameters: &[
            "`op` (string, **required**) - list, add, remove, enable, disable, or status",
            "`id` (string, required for add/remove/enable/disable) - stable file-tail source id",
            "`path` (string, required for add) - local log file path",
            "`tag` (string, required for add) - app/tag stored on ingested rows",
            "`host`, `facility`, `severity`, `start_at_end` (optional) - row envelope defaults",
        ],
    },
    AdminActionHelp {
        action: "notifications_test",
        description: "Send a test notification via the server-configured Apprise URLs. Rate-limited to 10 per minute per actor.\nCaller-supplied Apprise URLs are ignored for security; the server uses its own configured URLs.",
        parameters: &["`body` (string, optional) - notification body text (default: test message)"],
    },
];

fn admin_action_help() -> String {
    let mut help = String::new();
    for action in ADMIN_ACTION_HELP {
        help.push_str("---\n\n");
        help.push_str("## cortex ");
        help.push_str(action.action);
        help.push('\n');
        help.push_str(action.description);
        help.push_str("\n\n**Parameters:**\n");
        for parameter in action.parameters {
            help.push_str("- ");
            help.push_str(parameter);
            help.push('\n');
        }
        help.push('\n');
    }
    help
}

pub(super) async fn tool_cortex_help() -> anyhow::Result<Value> {
    let mut cheap = Vec::new();
    let mut moderate = Vec::new();
    let mut expensive = Vec::new();
    let mut write = Vec::new();
    for spec in actions::ACTION_SPECS {
        match spec.cost {
            actions::Cost::Cheap => cheap.push(spec.name),
            actions::Cost::Moderate => moderate.push(spec.name),
            actions::Cost::Expensive => expensive.push(spec.name),
            actions::Cost::Write => write.push(spec.name),
        }
    }
    let cost_guide = format!(
        r#"## Agent Planning Cost Metadata

Use action cost metadata to keep first-class agents token-efficient:
- `cheap`: {}.
- `moderate`: {}.
- `expensive`: {}.
- `write`: {}.

Recommended flow: start with cheap bounded calls, use moderate actions after
the scope is narrowed, and reserve expensive actions for a specific unanswered
question. Write actions require admin scope and must never be used for read-only
diagnosis.

"#,
        cheap.join(", "),
        moderate.join(", "),
        expensive.join(", "),
        write.join(", ")
    );
    let mut help = String::from(
        "# cortex Tool Reference\n\nThe MCP server exposes one tool, `cortex`. Set the required `action` argument to select the operation.\n\n",
    );
    for spec in actions::ACTION_SPECS {
        help.push_str("## cortex ");
        help.push_str(spec.name);
        help.push('\n');
        help.push_str(spec.description);
        help.push_str("\n\n");
        help.push_str("**Cost:** ");
        help.push_str(spec.cost.as_str());
        help.push_str("\n\n");
        if spec.flags.is_empty() {
            help.push_str("**Parameters:** none\n\n");
        } else {
            help.push_str("**Parameters:**\n");
            for flag in spec.flags {
                help.push_str("- `");
                help.push_str(flag.flag);
                help.push('`');
                if !flag.short.is_empty() {
                    help.push_str(" / `");
                    help.push_str(flag.short);
                    help.push('`');
                }
                help.push_str(" - ");
                help.push_str(flag.help);
                help.push('\n');
            }
            help.push('\n');
        }
        if !spec.examples.is_empty() {
            help.push_str("**Examples:**\n");
            for example in spec.examples {
                help.push_str("- `");
                help.push_str(example);
                help.push_str("`\n");
            }
            help.push('\n');
        }
        help.push_str("---\n\n");
    }
    let help = format!(
        "{help}\n{cost_guide}{}{}",
        admin_action_help(),
        r#"---

## cortex help
Returns this markdown documentation.

**Parameters:** none
"#
    );
    Ok(json!({ "help": help }))
}

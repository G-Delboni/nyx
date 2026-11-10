//! Merge/unmerge of the Nyx hook entries in a Claude Code `settings.json` value.
//! Pure functions over `serde_json::Value`; file I/O and backups live in `nyx-hook`.
//! Existing entries (e.g. the Orca hooks) are never removed or altered.

use serde_json::{json, Value};

/// Hooks the Nyx subscribes to.
pub const NYX_HOOK_EVENTS: [&str; 7] = [
    "Stop",
    "SubagentStop",
    "Notification",
    "PreToolUse",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
];

/// Hook timeout in seconds (Claude Code unit). `nyx-hook` itself gives up at 1.5 s.
pub const HOOK_TIMEOUT_SECS: u64 = 5;

/// Marker that identifies Nyx entries: the command contains the binary name.
pub const NYX_MARKER: &str = "nyx-hook";

pub fn is_nyx_command(command: &str) -> bool {
    command.to_ascii_lowercase().contains(NYX_MARKER)
}

/// Full command line for one hook: `"<exe>" event <Hook>`. Backslashes become forward
/// slashes (Claude Code runs hooks through a POSIX-style shell on Windows).
pub fn command_for(hook_command: &str, event: &str) -> String {
    let exe = hook_command.replace('\\', "/");
    if exe.contains(' ') && !exe.starts_with('"') {
        format!("\"{exe}\" event {event}")
    } else {
        format!("{exe} event {event}")
    }
}

fn entry_is_nyx(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(|c| c.as_str())
        .map(is_nyx_command)
        .unwrap_or(false)
}

/// Adds the Nyx entries for each hook in `NYX_HOOK_EVENTS`. Idempotent: if a Nyx
/// entry already exists for an event, its command is refreshed in place instead of
/// adding another one. Everything else in `settings` is preserved.
pub fn install(mut settings: Value, hook_command: &str) -> Value {
    if !settings.is_object() {
        settings = json!({});
    }
    let root = settings.as_object_mut().expect("object");
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        // Unexpected shape: do not destroy it.
        return settings;
    }
    let hooks = hooks.as_object_mut().expect("object");

    for event in NYX_HOOK_EVENTS {
        let command = command_for(hook_command, event);
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else {
            continue; // unexpected shape for this event: leave it alone
        };
        let mut found = false;
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                for h in list.iter_mut() {
                    if entry_is_nyx(h) {
                        found = true;
                        h["command"] = json!(command);
                        h["timeout"] = json!(HOOK_TIMEOUT_SECS);
                    }
                }
            }
        }
        if !found {
            let mut group = json!({
                "hooks": [{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS }]
            });
            if event == "PreToolUse" {
                group["matcher"] = json!("*");
            }
            groups.push(group);
        }
    }
    settings
}

/// Removes only the Nyx entries. Empty groups, empty events and an emptied `hooks`
/// object that the removal itself produced are dropped.
pub fn uninstall(mut settings: Value) -> Value {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return settings;
    };
    let mut emptied_events = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        let mut touched = false;
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                let before = list.len();
                list.retain(|h| !entry_is_nyx(h));
                touched |= list.len() != before;
            }
        }
        if touched {
            groups.retain(|g| {
                g.get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|l| !l.is_empty())
                    .unwrap_or(true)
            });
            if groups.is_empty() {
                emptied_events.push(event.clone());
            }
        }
    }
    for e in emptied_events {
        hooks.remove(&e);
    }
    if hooks.is_empty() {
        if let Some(root) = settings.as_object_mut() {
            root.remove("hooks");
        }
    }
    settings
}

/// True if any Nyx entry is present.
pub fn is_installed(settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.as_object())
        .map(|hooks| {
            hooks.values().any(|groups| {
                groups.as_array().map_or(false, |gs| {
                    gs.iter().any(|g| {
                        g.get("hooks")
                            .and_then(|h| h.as_array())
                            .map_or(false, |l| l.iter().any(entry_is_nyx))
                    })
                })
            })
        })
        .unwrap_or(false)
}

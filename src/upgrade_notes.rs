//! Release history is descriptive data. Commands come only from this allowlist;
//! displaying an upgrade step never executes it or reads a miner profile.
use crate::{Error, Result, need, update::version};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const ASSET: &str = "everycli-upgrade-notes.json";
pub const MAX_BYTES: usize = 1024 * 1024;
const BUNDLED: &[u8] = include_bytes!("../config/upgrade-notes.json");
const REPO: &str = "https://github.com/everyframe-studio/everyframe-miner";

fn tag(v: &str) -> String {
    if v.starts_with('v') {
        v.into()
    } else {
        format!("v{v}")
    }
}
fn text(v: &Value, limit: usize) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
}
fn fields(v: &Value, names: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == names.len() && names.iter().all(|k| o.contains_key(*k)))
}
pub fn command(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "worker-update" => "everycli worker-update",
        "reconnect" => "everycli reconnect",
        "apply-api-keys" => "everycli apply-api-keys",
        "reconcile" => "everycli reconcile",
        "review-offers" => "everycli offers",
        "retry-init" => "everycli init",
        "health" => "everycli health",
        "restart-ui" => "everycli ui",
        _ => return None,
    })
}
fn requirement(v: &Value) -> bool {
    matches!(v.as_str(), Some("required" | "conditional" | "recommended"))
}
pub fn catalog(bytes: &[u8]) -> Result<Value> {
    need(bytes.len() <= MAX_BYTES, "upgrade_notes_too_large")?;
    let v: Value = serde_json::from_slice(bytes).map_err(|_| Error("invalid_upgrade_notes"))?;
    need(
        fields(&v, &["schema", "firstRelease", "releases"]) && v["schema"] == 1,
        "invalid_upgrade_notes",
    )?;
    let releases = v["releases"]
        .as_array()
        .ok_or(Error("invalid_upgrade_notes"))?;
    need(
        !releases.is_empty() && releases.len() <= 1000,
        "invalid_upgrade_notes",
    )?;
    let mut tags = BTreeSet::new();
    for r in releases {
        need(
            fields(
                r,
                &["version", "title", "changes", "workerUpdate", "actions"],
            ) && text(&r["title"], 300),
            "invalid_upgrade_notes",
        )?;
        let t = r["version"]
            .as_str()
            .ok_or(Error("invalid_upgrade_notes"))?;
        version(t)?;
        need(tags.insert(t), "duplicate_upgrade_notes")?;
        need(
            matches!(
                r["workerUpdate"].as_str(),
                Some("required" | "conditional" | "recommended" | "not_required" | "unknown")
            ),
            "invalid_upgrade_notes",
        )?;
        let changes = r["changes"]
            .as_array()
            .ok_or(Error("invalid_upgrade_notes"))?;
        need(
            !changes.is_empty() && changes.len() <= 20 && changes.iter().all(|c| text(c, 2000)),
            "invalid_upgrade_notes",
        )?;
        let actions = r["actions"]
            .as_array()
            .ok_or(Error("invalid_upgrade_notes"))?;
        need(actions.len() <= 20, "invalid_upgrade_notes")?;
        for a in actions {
            need(
                fields(a, &["kind", "requirement", "when"])
                    && requirement(&a["requirement"])
                    && text(&a["when"], 1000)
                    && a["kind"].as_str().and_then(command).is_some(),
                "invalid_upgrade_notes",
            )?;
        }
        if matches!(
            r["workerUpdate"].as_str(),
            Some("required" | "conditional" | "recommended")
        ) {
            need(
                actions
                    .iter()
                    .any(|a| a["kind"] == "worker-update" && a["requirement"] == r["workerUpdate"]),
                "missing_worker_upgrade_step",
            )?;
        }
        need(
            !actions.iter().any(|a| {
                a["kind"] == "worker-update"
                    && a["requirement"] == "required"
                    && r["workerUpdate"] != "required"
            }),
            "conflicting_worker_upgrade_step",
        )?;
    }
    let first = v["firstRelease"]
        .as_str()
        .ok_or(Error("invalid_upgrade_notes"))?;
    let earliest = releases
        .iter()
        .filter_map(|r| r["version"].as_str())
        .min_by_key(|t| version(t).unwrap())
        .unwrap();
    need(first == earliest, "invalid_upgrade_notes_coverage")?;
    Ok(v)
}
fn rank(v: &Value) -> u8 {
    match v.as_str() {
        Some("required") => 3,
        Some("conditional") => 2,
        Some("recommended") => 1,
        _ => 0,
    }
}
fn add_action(actions: &mut Vec<Value>, a: &Value, release: &str) {
    if let Some(found) = actions.iter_mut().find(|x| x["kind"] == a["kind"]) {
        if rank(&a["requirement"]) > rank(&found["requirement"]) {
            found["requirement"] = a["requirement"].clone();
        }
        for (field, value) in [
            ("conditions", a["when"].clone()),
            ("versions", json!(release)),
        ] {
            let list = found[field].as_array_mut().unwrap();
            if !list.contains(&value) {
                list.push(value);
            }
        }
    } else {
        actions.push(
            json!({"kind":a["kind"],"command":command(a["kind"].as_str().unwrap()).unwrap(),
            "requirement":a["requirement"],"conditions":[a["when"]],"versions":[release]}),
        );
    }
}
pub fn summarize(bytes: &[u8], current: &str, target: &str, source: &str) -> Result<Value> {
    let v = catalog(bytes)?;
    let from = tag(current);
    let to = tag(target);
    let low = version(&from)?;
    let high = version(&to)?;
    need(high > low, "invalid_upgrade_range")?;
    let mut entries: Vec<Value> = v["releases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            let n = version(r["version"].as_str().unwrap()).unwrap();
            n > low && n <= high
        })
        .cloned()
        .collect();
    entries.sort_by_key(|r| version(r["version"].as_str().unwrap()).unwrap());
    need(
        entries.last().is_some_and(|r| r["version"] == to),
        "upgrade_notes_target_missing",
    )?;
    let complete = low >= version(v["firstRelease"].as_str().unwrap())?;
    let mut actions = vec![];
    let mut worker = "not_required";
    for r in &mut entries {
        let release = r["version"].as_str().unwrap().to_owned();
        r["url"] = json!(format!("{REPO}/releases/tag/{release}"));
        let state = r["workerUpdate"].as_str().unwrap();
        if state == "unknown" && worker == "not_required" {
            worker = "unknown";
        }
        if rank(&json!(state)) > rank(&json!(worker)) {
            worker = match state {
                "required" => "required",
                "conditional" => "conditional",
                _ => "recommended",
            };
        }
        for a in r["actions"].as_array().unwrap() {
            add_action(&mut actions, a, &release);
        }
    }
    add_action(
        &mut actions,
        &json!({"kind":"restart-ui","requirement":"conditional",
        "when":"If the dashboard is running, close it and reopen everycli ui to load the installed CLI."}),
        &to,
    );
    let result = json!({"status":if complete {"complete"} else {"partial"},"source":source,"from":from,"to":to,
        "history":entries,"actions":actions,"workerUpdate":if !complete && worker=="not_required" {"unknown"} else {worker},
        "message":if complete {String::new()} else {format!("History before {} is not included; review earlier release instructions.",v["firstRelease"].as_str().unwrap())},
        "releaseUrl":format!("{REPO}/releases/tag/{to}")});
    need(
        serde_json::to_vec(&result).is_ok_and(|b| b.len() <= 2 * MAX_BYTES),
        "upgrade_notes_too_large",
    )?;
    Ok(result)
}
pub fn bundled(current: &str, target: &str) -> Value {
    summarize(BUNDLED, current, target, "bundled").unwrap_or_else(|_| {
        let to = tag(target);
        json!({"status":"unavailable","source":"unavailable","from":tag(current),"to":to,
            "history":[],"actions":[{"kind":"restart-ui","command":"everycli ui","requirement":"conditional",
                "conditions":["If the dashboard is running, close it and reopen everycli ui to load the installed CLI."],"versions":[to]}],"workerUpdate":"unknown",
            "message":"Upgrade history could not be verified. Review the release instructions before changing your worker.",
            "releaseUrl":format!("{REPO}/releases/tag/{to}")})
    })
}
pub fn current_release(current: &str) -> Value {
    let Ok(catalog) = catalog(BUNDLED) else {
        return Value::Null;
    };
    let version = tag(current);
    let Some(mut release) = catalog["releases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["version"] == version)
        .cloned()
    else {
        return Value::Null;
    };
    release["url"] = json!(format!("{REPO}/releases/tag/{version}"));
    release
}
pub fn render(v: &Value) -> String {
    if !v.is_object() {
        return String::new();
    }
    let mut out = format!(
        "\nChanges from {} to {}:\n",
        v["from"].as_str().unwrap_or("—"),
        v["to"].as_str().unwrap_or("—")
    );
    if let Some(entries) = v["history"].as_array() {
        for r in entries {
            out.push_str(&format!(
                "\n{} — {}\n",
                r["version"].as_str().unwrap_or("—"),
                r["title"].as_str().unwrap_or("")
            ));
            if let Some(changes) = r["changes"].as_array() {
                for c in changes {
                    out.push_str(&format!("  • {}\n", c.as_str().unwrap_or("")));
                }
            }
        }
    }
    if let Some(message) = v["message"].as_str().filter(|s| !s.is_empty()) {
        out.push_str(&format!("\n{message}\n"));
    }
    out.push_str(match v["workerUpdate"].as_str() {
        Some("required") => "\nWorker update required: everycli worker-update\n",
        Some("conditional") => "\nWorker update needed for the features/providers listed below.\n",
        Some("recommended") => "\nWorker update recommended; review the steps below.\n",
        Some("not_required") => "\nNo new worker image is required by these CLI changes.\n",
        _ => "\nWorker update requirement is unknown; review the release instructions.\n",
    });
    if let Some(actions) = v["actions"].as_array().filter(|a| !a.is_empty()) {
        out.push_str("\nNext steps (not run automatically):\n");
        for a in actions {
            out.push_str(&format!(
                "  {}: {}\n",
                a["requirement"].as_str().unwrap_or("conditional"),
                a["command"].as_str().unwrap_or("")
            ));
            if let Some(conditions) = a["conditions"].as_array() {
                for c in conditions {
                    out.push_str(&format!("    {}\n", c.as_str().unwrap_or("")));
                }
            }
        }
    }
    out.push_str(&format!(
        "\nRelease notes: {}\n",
        v["releaseUrl"].as_str().unwrap_or(REPO)
    ));
    out
}

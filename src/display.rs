use serde_json::Value;

fn text(v: &Value) -> String {
    v.as_str()
        .unwrap_or("—")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}
fn provider_name(value: &Value) -> String {
    let key = value.as_str().unwrap_or("");
    crate::onboarding::credential_providers()
        .into_iter()
        .find(|p| crate::onboarding::credential_key(p).is_ok_and(|k| k == key))
        .unwrap_or("provider")
        .to_string()
}
fn version(value: &Value) -> String {
    crate::update::display_version(&text(value))
}
fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            rows.iter()
                .map(|r| r[i].chars().count())
                .max()
                .unwrap_or(0)
                .max(h.len())
        })
        .collect();
    let line = |r: &[String]| {
        r.iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{}{}",
                    s,
                    " ".repeat(widths[i].saturating_sub(s.chars().count()))
                )
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let mut out = vec![
        line(&headers.iter().map(|h| h.to_string()).collect::<Vec<_>>()),
        widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("  "),
    ];
    out.extend(rows.iter().map(|r| line(r)));
    if rows.is_empty() {
        out.push("No entries yet.".into());
    }
    out.join("\n")
}

pub fn render(command: &str, value: &Value) -> String {
    if value["ok"] == false && value["message"].is_string() {
        // These messages are fixed local diagnostics, never raw provider responses.
        return format!(
            "{}{}\n\n{}\n\nCode: {}\n",
            value["message"].as_str().unwrap_or("Profile unavailable."),
            value["file"]
                .as_str()
                .map(|p| format!("\nFile: {}", crate::cli_errors::clean(p)))
                .unwrap_or_default(),
            value["next"].as_str().unwrap_or(""),
            text(&value["error"])
        );
    }
    let rows = |key: &str| value[key].as_array().cloned().unwrap_or_default();
    match command {
        "setup" if value["nextSteps"].is_array() => {
            let mut out = format!(
                "Finish miner setup — steps 5–11\nLocal phase: {}\nDeployment alone does not make the miner ready for work. This is not a live readiness check. Use the same --network and --state-dir for every command below.\n\n",
                text(&value["phase"])
            );
            for (index, step) in rows("nextSteps").iter().enumerate() {
                out.push_str(&format!(
                    "{}. {}\n\n",
                    index + 5,
                    step.as_str().unwrap_or("")
                ));
            }
            out.push_str(value["next"].as_str().unwrap_or(""));
            out
        }
        "register-hotkey" => {
            let registered = value["registered"] == true;
            let mut out = format!(
                "Hotkey {} on {} SN{}.\nHotkey: {}\n",
                if registered {
                    "registered"
                } else {
                    "not registered"
                },
                text(&value["network"]),
                value["netuid"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or("—".into()),
                text(&value["hotkey"])
            );
            if let Some(uid) = value["uid"].as_u64() {
                out.push_str(&format!("UID: {uid}\n"));
            }
            if !registered && let Some(command) = value["registrationCommand"].as_str() {
                out.push_str(&format!("\nRegister using the wallet that owns this hotkey (fees may apply):\n{command}\n"));
                let placeholders = ["<wallet>", "<hotkey>"]
                    .into_iter()
                    .filter(|name| command.contains(name))
                    .collect::<Vec<_>>();
                if !placeholders.is_empty() {
                    out.push_str(&format!(
                        "Replace {} with the owning wallet names.\n",
                        placeholders.join(" and ")
                    ));
                }
            }
            out.push_str("No chain transaction was submitted and no registration fee was paid by everycli.\n");
            out.push_str(value["next"].as_str().unwrap_or(""));
            out
        }
        "set-api-keys" | "remove-api-key" => {
            let names = rows("updated")
                .iter()
                .map(provider_name)
                .collect::<Vec<_>>();
            let mut out = if value["removed"].is_string() {
                format!(
                    "Removed {} key from this local profile.\n",
                    provider_name(&value["removed"])
                )
            } else if value["notSaved"].is_string() {
                format!(
                    "No {} key is saved in this local profile. Nothing removed.\n",
                    provider_name(&value["notSaved"])
                )
            } else if names.is_empty() {
                "No keys changed. Existing keys were kept; empty entries were skipped.\n".into()
            } else {
                format!("Saved locally: {}.\n", names.join(", "))
            };
            out.push_str("Running workers are unchanged.\n");
            out.push_str(value["next"].as_str().unwrap_or(""));
            out
        }
        "self-update" => {
            if value["updated"] == true {
                format!(
                    "Updated everycli from {} to {}.\nProfiles and deployed workers are unchanged.",
                    version(&value["previous"]),
                    version(&value["version"])
                )
            } else {
                format!(
                    "Installed: {}\nLatest: {}\n{}",
                    version(&value["current"]),
                    version(&value["latest"]),
                    if value["updateAvailable"] == true {
                        "Update available. Run everycli update to install it."
                    } else {
                        "No update available. The installed CLI was not changed."
                    }
                )
            }
        }
        "providers" => {
            let data = rows("providers")
                .iter()
                .map(|r| {
                    vec![
                        text(&r["provider"]),
                        if r["configured"] == true {
                            "Saved"
                        } else {
                            "Missing"
                        }
                        .into(),
                        match r["releaseAllows"].as_bool() {
                            Some(true) => "Allowed",
                            Some(false) => "Not allowed",
                            None => "Unknown",
                        }
                        .into(),
                        text(&r["reason"]),
                    ]
                })
                .collect();
            format!(
                "{}\n\nPhala hosting token: {}\n{}\n",
                table(&["PROVIDER", "LOCAL KEY", "RELEASE", "REASON"], data),
                if value["phalaConfigured"] == true {
                    "saved"
                } else {
                    "missing (Settings → API Tokens, not Private AI)"
                },
                text(&value["note"])
            )
        }
        "offers" => {
            let catalog = value["catalogAvailable"] == true;
            let data = rows("offers")
                .iter()
                .map(|r| {
                    let mut row = vec![
                        text(&r["model"]),
                        if r["offered"] == true {
                            if r["pricingCurrent"] == true {
                                "Active"
                            } else {
                                "Stale pricing"
                            }
                        } else if catalog && r["offerable"] == false {
                            "Unavailable"
                        } else {
                            "Inactive"
                        }
                        .into(),
                        r["discountBp"]
                            .as_u64()
                            .filter(|n| *n <= 9990)
                            .map(|n| format!("{:.2}%", n as f64 / 100.0))
                            .unwrap_or("—".into()),
                    ];
                    if catalog {
                        row.insert(1, text(&r["provider"]));
                        row.push(crate::offer_catalog::readiness(r).into());
                    }
                    row
                })
                .collect();
            format!(
                "{}\n\nDiscount is off the base miner reward, not your provider's API price.\n{}",
                table(
                    if catalog {
                        &["MODEL", "PROVIDER", "OFFER", "DISCOUNT", "READINESS"]
                    } else {
                        &["MODEL", "OFFER", "DISCOUNT"]
                    },
                    data
                ),
                if catalog {
                    "Catalog entries are not automatically enabled. A saved API key does not update your deployed worker. Integration/pricing/release requirements are coordinator-side; use --json for all blockers and contract limits. EveryFrame Local is not an API model.\n"
                } else {
                    "This coordinator returned only its allowlisted models, not the full Studio catalog.\n"
                }
            )
        }
        "balances" => {
            let data = rows("balances")
                .iter()
                .map(|r| {
                    let age = r["checkedAt"]
                        .as_i64()
                        .filter(|n| *n > 0)
                        .map(|n| format!("{}s ago", (crate::now() - n).max(0) / 1000))
                        .unwrap_or("—".into());
                    vec![
                        text(&r["provider"]),
                        r["remainingUsd"]
                            .as_f64()
                            .map(|n| format!("${n:.2}"))
                            .unwrap_or("—".into()),
                        if r["stale"] == true {
                            format!("Stale / {}", text(&r["status"]))
                        } else {
                            text(&r["status"])
                        },
                        text(&r["source"]),
                        age,
                    ]
                })
                .collect();
            let mut out = table(
                &["PROVIDER", "REMAINING USD", "STATUS", "SOURCE", "CHECKED"],
                data,
            );
            out.push_str("\n\nAccount-level balances may be shared across miners; do not add them together.\nPhala shows credited funds, excluding post-paid limits and outstanding invoices.\nUnavailable: check billing permissions or provider connectivity. Unsupported: no supported USD balance query.\n");
            for r in rows("balances") {
                if r["status"] == "unavailable" || r["status"] == "unsupported" {
                    let code = r["diagnostic"]["code"].as_str().unwrap_or(
                        if r["status"] == "unsupported" {
                            "unsupported_balance"
                        } else if r["source"] == "synced" {
                            "balance_snapshot_reason_unavailable"
                        } else {
                            "request_failed"
                        },
                    );
                    let d = crate::balances::diagnostic(r["provider"].as_str().unwrap_or(""), code);
                    out.push_str(&format!(
                        "\n{} [{}]: {}\n  {}\n",
                        text(&r["provider"]),
                        text(&d["code"]),
                        d["message"].as_str().unwrap(),
                        d["next"].as_str().unwrap()
                    ));
                }
            }
            if value["published"] == true {
                out.push_str(
                    "Synced to this miner's private coordinator view. No API keys were sent.\n",
                );
            }
            if value["remoteUnavailable"] == true {
                out.push_str("Coordinator snapshot unavailable; showing local results only.\n");
                let d = crate::diagnostics::value(
                    value["remoteError"]["code"]
                        .as_str()
                        .unwrap_or("request_failed"),
                );
                out.push_str(&format!(
                    "Coordinator [{}]: {} {}\n",
                    text(&d["code"]),
                    d["message"].as_str().unwrap(),
                    d["next"].as_str().unwrap()
                ));
            }
            if rows("balances").is_empty() {
                out.push_str("No balance rows available. Configure local keys with everycli set-api-keys, or refresh the snapshot from your key-holding device.\n");
                out.push_str("On the device holding API keys, run: everycli balances --publish\n");
            }
            if rows("balances").iter().any(|r| r["stale"] == true) {
                out.push_str("Stale means older than 15 minutes or missing a valid timestamp; these are not live balances. Run everycli balances --publish on the device holding the API keys to refresh synced snapshots.\n");
            }
            out
        }
        _ => serde_json::to_string_pretty(value).unwrap(),
    }
}

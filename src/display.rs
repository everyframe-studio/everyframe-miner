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

// Explicit state words survive pipes, monochrome terminals and screen readers.
// Add ANSI only after computing widths; --json never passes through this path.
pub fn render_with_color(command: &str, value: &Value, color: bool) -> String {
    let mut out = render(command, value);
    if ["health", "status", "doctor"].contains(&command) && value["label"].is_string() {
        out = format!("Worker: {}\n\n{out}", text(&value["label"]));
    }
    if command == "status" && value["authorization"]["nonExpiring"] == false {
        out.push_str(&format!(
            "\nAuthorization expires in {} days. {}\n",
            value["authorization"]["remainingDays"],
            text(&value["authorization"]["migration"])
        ));
    }
    if color && ["health", "status"].contains(&command) {
        for (label, code) in [
            ("[PASS]", 32),
            ("[FAIL]", 31),
            ("[WARN]", 33),
            ("[INFO]", 36),
        ] {
            out = out.replace(label, &format!("\x1b[{code}m{label}\x1b[0m"));
        }
    }
    out
}

fn count(value: &Value) -> String {
    value
        .as_u64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "Not reported".into())
}

fn wrapped(value: &str, width: usize) -> Vec<String> {
    let clean: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(1600)
        .collect();
    let mut lines = vec![String::new()];
    for word in clean.split_whitespace() {
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > width {
            lines.push(String::new());
        }
        // Long model identifiers must not stretch the whole terminal table.
        for ch in word.chars() {
            let line = lines.last_mut().unwrap();
            if line.chars().count() >= width {
                lines.push(String::new());
            }
            lines.last_mut().unwrap().push(ch);
        }
        lines.last_mut().unwrap().push(' ');
    }
    lines.iter().map(|s| s.trim_end().to_string()).collect()
}

fn check_table(checks: &[Value]) -> String {
    let mut rows = vec![];
    for check in checks {
        let state = match check["state"].as_str() {
            Some("PASS") => "[PASS]",
            Some("WARN") => "[WARN]",
            Some("INFO") => "[INFO]",
            Some("FAIL") => "[FAIL]",
            _ if check["ok"] == true => "[PASS]",
            _ => "[FAIL]",
        };
        let names = wrapped(check["name"].as_str().unwrap_or("Check"), 28);
        let details = wrapped(check["detail"].as_str().unwrap_or("Not reported"), 68);
        for i in 0..names.len().max(details.len()) {
            rows.push(vec![
                names.get(i).cloned().unwrap_or_default(),
                if i == 0 { state.into() } else { String::new() },
                details.get(i).cloned().unwrap_or_default(),
            ]);
        }
    }
    table(&["CHECK", "STATE", "DETAIL"], rows)
}

fn bool_check(name: &str, value: &Value, expected: bool, yes: &str, no: &str) -> Value {
    let state = match value.as_bool() {
        Some(v) if v == expected => "PASS",
        Some(_) => "FAIL",
        None => "WARN",
    };
    serde_json::json!({"name":name,"state":state,"detail":match state {
        "PASS" => yes, "FAIL" => no, _ => "Not reported; cannot verify this check."
    }})
}

fn activity_table(value: &Value) -> String {
    let mut out = table(
        &["ACTIVITY", "COUNT"],
        vec![
            vec!["Active jobs".into(), count(&value["activeJobs"])],
            vec![
                "Historically accepted jobs".into(),
                count(&value["acceptedJobs"]),
            ],
        ],
    );
    if let Some(rows) = value["jobs"].as_array() {
        let rows = rows
            .iter()
            .filter(|r| r["state"] != "accepted")
            .map(|r| vec![text(&r["state"]), count(&r["count"])])
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            out.push_str(&format!(
                "\n\n{}",
                table(&["OTHER JOB STATES (HISTORICAL)", "COUNT"], rows)
            ));
        }
    }
    out.push_str("\n\nZero active jobs is normal when idle. Historical acceptance is not a live test of every model.\nNo paid generation or API balance check was performed.");
    out
}

fn health_table(value: &Value) -> String {
    let checks = value["checks"].as_array().cloned().unwrap_or_default();
    let all_pass = !checks.is_empty()
        && value["ok"] == true
        && checks.iter().all(|c| {
            c["ok"] == true && !["WARN", "FAIL"].contains(&c["state"].as_str().unwrap_or(""))
        });
    let failed = checks
        .iter()
        .any(|c| c["ok"] != true && c["state"] != "WARN");
    let mut out = format!(
        "Miner health — {}\n\n{}\n\n{}\n",
        if all_pass {
            "[PASS] All readiness checks passed"
        } else if failed {
            "[FAIL] Readiness needs attention"
        } else {
            "[WARN] Readiness could not be fully verified"
        },
        check_table(&checks),
        activity_table(&value["activity"])
    );
    if all_pass {
        out.push_str("Ready to accept work based on this snapshot. This does not guarantee traffic, provider funds, successful future jobs or earnings.\n");
    } else {
        out.push_str("Resolve failed or unverified checks before calling the miner fully ready. No activation, resume or redeployment was performed.\n");
    }
    out
}

fn status_table(value: &Value) -> String {
    let remote = &value["coordinator"];
    let mut identity = vec![
        vec!["Miner".into(), text(&value["minerId"])],
        vec![
            "Network".into(),
            format!(
                "{} / SN{}",
                text(&value["network"]),
                count(&value["netuid"])
            ),
        ],
        vec!["Release".into(), text(&value["release"])],
        vec!["Local phase".into(), text(&value["phase"])],
        vec!["Local app".into(), text(&value["appId"])],
    ];
    if remote["appId"].is_string() {
        identity.push(vec!["Coordinator app".into(), text(&remote["appId"])]);
    }
    let mut checks = vec![bool_check(
        "Deployment configuration",
        &value["invitationExpired"],
        false,
        "Signed configuration is current.",
        "Signed configuration expired; renew with everycli init.",
    )];
    let coordinator_ok = remote.is_object() && value["coordinatorError"].is_null();
    checks.push(serde_json::json!({"name":"Coordinator","state":if coordinator_ok {"PASS"} else {"WARN"},
        "detail":if coordinator_ok {"Signed response verified.".into()} else {format!("Unavailable: {}. Run everycli doctor.", text(&value["coordinatorError"]["code"]))}}));
    let cloud_state = value["cloud"]["status"].as_str();
    checks.push(serde_json::json!({"name":"Phala workload","state":match cloud_state {Some("running")=>"PASS",Some(_)=>"FAIL",None=>"WARN"},
        "detail":if let Some(state) = cloud_state {format!("Cloud state: {state}")} else if !value["cloudError"].is_null() {
            format!("Cloud check failed: {}. Run everycli doctor.", text(&value["cloudError"]["code"]))
        } else {"Not checked from this profile. Use the original management profile; do not redeploy.".into()}}));
    if let Some(list) = value["cloudWorkloads"].as_array().filter(|l| !l.is_empty()) {
        for w in list {
            checks.push(serde_json::json!({"name":"Workload not managed here","state":"WARN",
                "detail":format!("{} ({}) carries this miner's name but was deployed from another computer or an earlier setup. Manage it there, or delete it with everycli delete --app-id {} before deploying from here.",
                    text(&w["appId"]), text(&w["status"]), text(&w["appId"]))}));
        }
    }
    checks.extend([
        bool_check(
            "Miner enabled",
            &remote["enabled"],
            true,
            "Enabled on coordinator.",
            "Disabled on coordinator.",
        ),
        bool_check(
            "Worker online",
            &remote["online"],
            true,
            "Coordinator reports worker online.",
            "Worker is offline.",
        ),
        bool_check(
            "Workload attested",
            &remote["attested"],
            true,
            "Coordinator reports an attested workload.",
            "Workload is not currently attested.",
        ),
        bool_check(
            "Routing enabled",
            &remote["routingEnabled"],
            true,
            "Miner routing enabled.",
            "Miner routing disabled.",
        ),
        bool_check(
            "Accepting new work",
            &remote["draining"],
            false,
            "Not draining; other readiness checks still apply.",
            "Draining or paused; not accepting new jobs.",
        ),
    ]);
    // Status is an overview, not a second implementation of doctor. Show time and
    // capability evidence, but never claim the full health checklist passed.
    checks.extend(crate::health::runtime_checks(remote, crate::now()));
    let offers = remote["offers"].as_array().cloned().unwrap_or_default();
    let offer_rows = offers
        .iter()
        .map(|r| {
            vec![
                text(&r["model"]),
                if r["offered"] == true {
                    if r["pricingCurrent"] == true {
                        "Active"
                    } else {
                        "Stale pricing"
                    }
                } else {
                    "Inactive"
                }
                .into(),
                r["discountBp"]
                    .as_u64()
                    .filter(|n| *n <= 9990)
                    .map(|n| format!("{:.2}%", n as f64 / 100.))
                    .unwrap_or_else(|| "—".into()),
            ]
        })
        .collect();
    format!(
        "Miner status\n\n{}\n\n{}\n\n{}\n\n{}\n\nRun everycli health for the full readiness checklist. The local phase alone does not prove readiness.\nUse everycli status --json for full fields; everycli offers for model blockers.\n",
        table(&["MINER", "VALUE"], identity),
        check_table(&checks),
        table(&["MODEL OFFER", "STATE", "DISCOUNT"], offer_rows),
        activity_table(&crate::health::activity(remote))
    )
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
        "auto-update" => {
            let enabled = if value["enabled"] == true {
                "On"
            } else {
                "Off"
            };
            let effective = if value["environmentDisabled"] == true {
                "Disabled by EVERYCLI_DISABLE_AUTO_UPDATE=1"
            } else if value["nativeInstallation"] != true {
                "Manual updates for this installation path"
            } else if value["effective"] == true {
                "Enabled for interactive commands"
            } else {
                "Disabled"
            };
            let last = &value["lastAttempt"];
            let status = if last.is_null() {
                "Not checked yet".into()
            } else {
                text(&last["state"])
            };
            let mut out = table(
                &["CLI AUTO-UPDATE", "STATUS"],
                vec![
                    vec!["Preference".into(), enabled.into()],
                    vec!["Effective mode".into(), effective.into()],
                    vec!["Check interval".into(), "6 hours".into()],
                    vec!["Last attempt".into(), status],
                ],
            );
            if !last["code"].is_null() {
                out.push_str(&format!(
                    "\nLast error: {}. Manual retry: everycli update.\n",
                    text(&last["code"])
                ));
            }
            if last["result"]["updated"] == true {
                out.push_str(&format!(
                    "\nInstalled {}. The new version is used on the next invocation.\n",
                    version(&last["result"]["version"])
                ));
            }
            out.push_str("\nCLI only: deployed workers, keys and offers are unchanged.\nDisable: everycli auto-update --disable · Enable: everycli auto-update --enable\n");
            out
        }
        "update" => format!(
            "Worker update — {}\n{}{}\n{}\n",
            text(value.get("state").unwrap_or(&value["phase"])),
            value["release"]
                .as_str()
                .map(|v| format!("Release: {}\n", crate::cli_errors::clean(v)))
                .unwrap_or_default(),
            value["reason"]
                .as_str()
                .map(|v| format!("{}\n", crate::cli_errors::clean(v)))
                .unwrap_or_default(),
            value["next"]
                .as_str()
                .unwrap_or("Run everycli status to inspect the worker.")
        ),
        "health" => health_table(value),
        "status" => status_table(value),
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
            if value["discovery"] == true {
                let data = rows("offers")
                    .iter()
                    .map(|r| {
                        vec![
                            text(&r["model"]),
                            text(&r["provider"]),
                            r["pricing"]["minerRewardMicrousd"]
                                .as_f64()
                                .map(|v| format!("${:.6}", v / 1_000_000.))
                                .unwrap_or("Not priced".into()),
                        ]
                    })
                    .collect();
                return format!(
                    "{}\n\n{}\n{}\n",
                    table(&["MODEL", "PROVIDER", "BASE WORK VALUE"], data),
                    text(&value["note"]),
                    text(&value["next"])
                );
            }
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
                        if r["nativeBalance"]["unit"] == "W_CREDITS" {
                            r["nativeBalance"]["remainingMicrounits"]
                                .as_i64()
                                .map(|n| format!("{:.3} W", n as f64 / 1_000_000.0))
                                .unwrap_or("—".into())
                        } else {
                            r["remainingUsd"]
                                .as_f64()
                                .map(|n| format!("${n:.2}"))
                                .unwrap_or("—".into())
                        },
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
                &["PROVIDER", "REMAINING", "STATUS", "SOURCE", "CHECKED"],
                data,
            );
            out.push_str("\n\nAccount-level balances may be shared across miners; do not add them together.\nVivix shows W credits, not A credits or USD. Phala shows credited funds, excluding post-paid limits and outstanding invoices.\nUnavailable: check billing permissions or provider connectivity. Unsupported: balance lookup is not available in this snapshot.\n");
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

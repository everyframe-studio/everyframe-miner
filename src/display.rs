use serde_json::Value;

fn text(v: &Value) -> String {
    v.as_str()
        .unwrap_or("—")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
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
    if matches!(command, "status" | "doctor") && value["ok"] == false && value["state"].is_string()
    {
        // These messages are fixed local diagnostics, never raw provider responses.
        return format!(
            "{}\n\n{}\n\nCode: {}\n",
            value["message"].as_str().unwrap_or("Profile unavailable."),
            value["next"].as_str().unwrap_or(""),
            text(&value["error"])
        );
    }
    let rows = |key: &str| value[key].as_array().cloned().unwrap_or_default();
    match command {
        "offers" => {
            let data = rows("offers")
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
                            .map(|n| format!("{:.2}%", n as f64 / 100.0))
                            .unwrap_or("—".into()),
                    ]
                })
                .collect();
            format!(
                "{}\n\nDiscount is off the base miner reward, not your provider's API price.\n",
                table(&["MODEL", "OFFER", "DISCOUNT"], data)
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
            if value["published"] == true {
                out.push_str(
                    "Synced to this miner's private coordinator view. No API keys were sent.\n",
                );
            }
            if value["remoteUnavailable"] == true {
                out.push_str("Coordinator snapshot unavailable; showing local results only.\n");
            }
            if rows("balances").is_empty() {
                out.push_str("On the device holding API keys, run: everycli balances --publish\n");
            }
            out
        }
        _ => serde_json::to_string_pretty(value).unwrap(),
    }
}

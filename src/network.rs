use crate::{Error, Result, need};
use serde_json::Value;
use std::{
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    time::Duration,
};
use url::Url;
#[derive(Clone)]
pub struct Request {
    pub url: String,
    pub hosts: Vec<String>,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub limit: usize,
    pub timeout: u64,
    pub redirect_hosts: Vec<String>,
    pub submit_deadline: Option<i64>,
}
impl Request {
    pub fn get(url: impl Into<String>, hosts: &[&str]) -> Self {
        Self {
            url: url.into(),
            hosts: hosts.iter().map(|s| s.to_string()).collect(),
            method: "GET".into(),
            headers: vec![],
            body: None,
            limit: 200000,
            timeout: 15,
            redirect_hosts: vec![],
            submit_deadline: None,
        }
    }
    pub fn json(mut self, method: &str, body: &Value) -> Self {
        self.method = method.into();
        self.headers
            .push(("content-type".into(), "application/json".into()));
        self.body = Some(body.to_string().into_bytes());
        self
    }
}
pub trait Http {
    fn bytes(&self, r: Request) -> Result<Vec<u8>>;
    fn dialogue(&self, _r: Request, _voices: &Value, _inputs: &Value) -> Result<Vec<u8>> {
        Err(Error("unsupported_stream_transport"))
    }
    fn json(&self, r: Request) -> Result<Value> {
        let result = serde_json::from_slice(&self.bytes(r)?);
        crate::execution_diagnostics::json_result(&result);
        result.map_err(|_| Error("invalid_remote_json"))
    }
    /// Independent, read-only account queries. Test transports may stay serial.
    fn read_json_batch(&self, requests: Vec<Request>) -> Vec<Result<Value>> {
        requests
            .into_iter()
            .map(|r| {
                if r.method != "GET" || r.body.is_some() {
                    Err(Error("invalid_method"))
                } else {
                    self.json(r)
                }
            })
            .collect()
    }
}
pub struct PublicHttp;

fn parallel_reads(
    requests: Vec<Request>,
    read: impl Fn(Request) -> Result<Value> + Sync,
) -> Vec<Result<Value>> {
    // Bound concurrency even if a future caller supplies a larger batch.
    let mut requests = requests.into_iter();
    let mut results = Vec::new();
    loop {
        let chunk: Vec<_> = requests.by_ref().take(8).collect();
        if chunk.is_empty() {
            break;
        }
        results.extend(std::thread::scope(|scope| {
            let read = &read;
            let workers: Vec<_> = chunk
                .into_iter()
                .map(|r| {
                    scope.spawn(move || {
                        if r.method != "GET" || r.body.is_some() {
                            Err(Error("invalid_method"))
                        } else {
                            read(r)
                        }
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap_or(Err(Error("transport_failed"))))
                .collect::<Vec<_>>()
        }));
    }
    results
}
fn worker_update_endpoint(u: &Url, method: &str) -> bool {
    u.host_str() == Some("subnet.everyframe.studio")
        && method == "POST"
        && ["/v1/miner/", "/mainnet/v1/miner/"].iter().any(|prefix| {
            u.path().strip_prefix(prefix).is_some_and(|action| {
                [
                    "worker-update-prepare",
                    "worker-rollback",
                    "worker-auto-update-policy",
                    "worker-auto-update-report",
                    "worker-auto-drain",
                    "worker-auto-resume",
                ]
                .contains(&action)
            })
        })
}
fn worker_update_error(status: u16, bytes: &[u8]) -> Error {
    if bytes.len() <= 4096
        && let Ok(v) = serde_json::from_slice::<Value>(bytes)
    {
        let code = match (status, v["error"].as_str()) {
            (409, Some("worker_auto_fleet_update_busy")) => "worker_auto_fleet_update_busy",
            (409, Some("worker_auto_update_lease_lost")) => "worker_auto_update_lease_lost",
            (409, Some("worker_auto_update_not_enabled")) => "worker_auto_update_not_enabled",
            (409, Some("worker_auto_update_bootstrap_required")) => {
                "worker_auto_update_bootstrap_required"
            }
            (409, Some("worker_auto_update_disable_first")) => "worker_auto_update_disable_first",
            (409, Some("worker_auto_update_recovery_pending")) => {
                "worker_auto_update_recovery_pending"
            }
            (409, Some("worker_recovery_unavailable")) => "worker_recovery_unavailable",
            (409, Some("worker_recovery_in_progress")) => "worker_recovery_in_progress",
            (409, Some("worker_recovery_not_idle")) => "worker_recovery_not_idle",
            (409, Some("worker_recovery_target_changed")) => "worker_recovery_target_changed",
            (409, Some("worker_update_target_changed")) => "worker_update_target_changed",
            (400, Some("invalid_updater_capsule")) => "invalid_updater_capsule",
            _ => return crate::diagnostics::http_error(status),
        };
        return Error(code);
    }
    crate::diagnostics::http_error(status)
}
fn history_error(status: u16, bytes: &[u8]) -> Error {
    if status == 400
        && bytes.len() <= 4096
        && let Ok(v) = serde_json::from_slice::<Value>(bytes)
    {
        return Error(match v["error"].as_str() {
            Some("invalid_history_cursor") => "invalid_history_cursor",
            Some("invalid_history_limit") => "invalid_history_limit",
            Some("invalid_history_status") => "invalid_history_status",
            Some("invalid_history_model") => "invalid_history_model",
            Some("invalid_history_query") => "invalid_history_query",
            _ => "http_400",
        });
    }
    crate::diagnostics::http_error(status)
}
fn batch_offer_error(status: u16, bytes: &[u8]) -> Error {
    if bytes.len() <= 4096
        && let Ok(v) = serde_json::from_slice::<Value>(bytes)
    {
        match (status, v["error"].as_str()) {
            (409, Some("automatic_offer_policy_changed")) => {
                return Error("automatic_offer_policy_changed");
            }
            (409, Some("automatic_offer_pricing_unsupported")) => {
                return Error("automatic_offer_pricing_unsupported");
            }
            (400, Some("invalid_automatic_offer_limits")) => {
                return Error("invalid_automatic_offer_limits");
            }
            (409, Some("offer_revision_conflict")) => return Error("offer_revision_conflict"),
            (409, Some("offer_pricing_changed")) => return Error("offer_pricing_changed"),
            (409, Some("provider_model_unavailable")) => {
                return Error("provider_model_unavailable");
            }
            (400, Some("invalid_offer_batch")) => return Error("invalid_offer_batch"),
            (403, Some("model_not_allowed")) => return Error("model_not_allowed"),
            _ => {}
        }
    }
    crate::diagnostics::http_error(status)
}
fn enrollment_error(status: u16, bytes: &[u8]) -> Error {
    if bytes.len() <= 4096
        && let Ok(v) = serde_json::from_slice::<Value>(bytes)
    {
        let code = match (status, v["error"].as_str()) {
            (503, Some("public_onboarding_unavailable")) => {
                "public_onboarding_unavailable_no_vm_created"
            }
            (403, Some("hotkey_not_registered")) => "hotkey_not_registered",
            (403, Some("miner_paused_contact_operator")) => "miner_paused_contact_operator",
            (409, Some("existing_miner_requires_migration")) => "existing_miner_requires_migration",
            (403, Some("authorization_revoked")) => "authorization_revoked",
            (503, Some("deployment_os_unavailable")) => "deployment_os_unavailable",
            (503, Some("deployment_catalog_unavailable")) => "deployment_catalog_unavailable",
            (503, Some("chain_lookup_unavailable")) => "chain_lookup_unavailable",
            (503, Some("enrollment_timeout")) => "enrollment_timeout",
            (429, Some("enrollment_busy")) => "enrollment_busy",
            (401, Some("enrollment_expired")) => "enrollment_expired",
            (401, Some("enrollment_domain_mismatch")) => "enrollment_domain_mismatch",
            (401, Some("hotkey_ownership_required")) => "hotkey_ownership_required",
            (409, Some("enrollment_replayed")) => "enrollment_replayed",
            (409, Some("recovery_target_changed")) => "recovery_target_changed",
            (409, Some("recovery_active_jobs")) => "recovery_active_jobs",
            (409, Some("recovery_worker_still_online")) => "recovery_worker_still_online",
            _ => return crate::diagnostics::http_error(status),
        };
        return Error(code);
    }
    crate::diagnostics::http_error(status)
}
pub fn safe_url(input: &str, hosts: &[String]) -> Result<Url> {
    safe_url_stage(input, hosts, "initial")
}
fn safe_url_stage(input: &str, hosts: &[String], stage: &str) -> Result<Url> {
    let u = Url::parse(input).map_err(|_| {
        crate::destination_diagnostics::capture(input, hosts, stage, "invalid_url");
        Error("unapproved_destination")
    })?;
    let host = u.host_str().unwrap_or("");
    let rule = if u.scheme() != "https" {
        Some("https_required")
    } else if !u.username().is_empty() || u.password().is_some() {
        Some("userinfo_disallowed")
    } else if u.port_or_known_default() != Some(443) {
        Some("port_disallowed")
    } else if u.fragment().is_some() {
        Some("fragment_disallowed")
    } else if !hosts.iter().any(|h| {
        h.strip_prefix("*.").map_or(host == h, |suffix| {
            host.ends_with(&format!(".{suffix}")) && host != suffix
        })
    }) {
        Some("host_not_allowed")
    } else {
        None
    };
    if let Some(rule) = rule {
        crate::destination_diagnostics::capture(input, hosts, stage, rule);
        return Err(Error("unapproved_destination"));
    }
    Ok(u)
}
pub fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let b = u32::from(ip);
            ![
                (0, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0a80000, 16),
                (0xc0000000, 24),
                (0xc6120000, 15),
                (0xe0000000, 4),
                (0xf0000000, 4),
            ]
            .iter()
            .any(|(base, bits)| b >> (32 - bits) == base >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
        }
    }
}
impl Http for PublicHttp {
    fn read_json_batch(&self, requests: Vec<Request>) -> Vec<Result<Value>> {
        parallel_reads(requests, |request| self.json(request))
    }
    fn dialogue(&self, r: Request, voices: &Value, inputs: &Value) -> Result<Vec<u8>> {
        crate::native_websocket::dialogue(r, voices, inputs)
    }
    fn bytes(&self, mut r: Request) -> Result<Vec<u8>> {
        let mut trace = crate::execution_diagnostics::HttpSpan::new(&r);
        let result = (|| {
            crate::destination_diagnostics::clear();
            for hop in 0..=2 {
                let u = safe_url_stage(
                    &r.url,
                    &r.hosts,
                    if hop == 0 { "initial" } else { "redirect" },
                )?;
                let host = u.host_str().ok_or(Error("unapproved_destination"))?;
                let dns_started = std::time::Instant::now();
                let addresses = (host, 443)
                    .to_socket_addrs()
                    .map_err(|_| {
                        crate::destination_diagnostics::capture(
                            &r.url,
                            &r.hosts,
                            if hop == 0 { "initial" } else { "redirect" },
                            "dns_failed",
                        );
                        Error("dns_failed")
                    })?
                    .collect::<Vec<_>>();
                trace.dns(dns_started.elapsed().as_millis() as u64);
                if addresses.is_empty() || !addresses.iter().all(|a| public_address(a.ip())) {
                    crate::destination_diagnostics::capture(
                        &r.url,
                        &r.hosts,
                        if hop == 0 { "initial" } else { "redirect" },
                        "private_address",
                    );
                    return Err(Error("private_destination"));
                }
                let client = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .timeout(Duration::from_secs(r.timeout))
                    .connect_timeout(Duration::from_secs(r.timeout))
                    .resolve_to_addrs(host, &addresses)
                    .pool_max_idle_per_host(0)
                    .build()
                    .map_err(|_| Error("transport_failed"))?;
                let method = reqwest::Method::from_bytes(r.method.as_bytes())
                    .map_err(|_| Error("invalid_method"))?;
                let mut request = client.request(method, u.clone());
                for (k, v) in &r.headers {
                    let mut value = reqwest::header::HeaderValue::from_str(v)
                        .map_err(|_| Error("invalid_header"))?;
                    value.set_sensitive(true);
                    request = request.header(k, value)
                }
                if let Some(b) = r.body.clone() {
                    request = request.body(b)
                }
                // DNS, reference verification and request construction can be slow.
                // Fence at the last boundary before any paid bytes leave the worker.
                if let Some(deadline) = r.submit_deadline {
                    need(crate::now() < deadline, "submission_deadline_elapsed")?;
                }
                let response = request
                    .send()
                    .map_err(|e| crate::diagnostics::transport_error(&e))?;
                trace.response(
                    response.status().as_u16(),
                    response
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok()),
                    response.content_length(),
                    crate::execution_diagnostics::public_request_id(
                        ["x-request-id", "request-id", "x-amzn-requestid", "cf-ray"]
                            .iter()
                            .find_map(|k| response.headers().get(*k).and_then(|v| v.to_str().ok())),
                        &r,
                    ),
                    response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()),
                );
                if [301, 302, 303, 307, 308].contains(&response.status().as_u16())
                    && r.method == "GET"
                    && !r.redirect_hosts.is_empty()
                    && hop < 2
                {
                    let location = response
                        .headers()
                        .get("location")
                        .and_then(|v| v.to_str().ok())
                        .ok_or(Error("invalid_redirect"))?;
                    let target = u.join(location).map_err(|_| Error("invalid_redirect"))?;
                    safe_url_stage(target.as_str(), &r.redirect_hosts, "redirect")?;
                    r.url = target.to_string();
                    r.hosts = r.redirect_hosts.clone();
                    r.headers.clear();
                    r.body = None;
                    continue;
                }
                if !response.status().is_success()
                    && u.host_str() == Some("subnet.everyframe.studio")
                    && u.path().contains("/onboarding/")
                {
                    // Only expose fixed, known enrollment errors, never arbitrary
                    // remote text or response bodies that might contain secrets.
                    let mut bytes = Vec::new();
                    let status = response.status().as_u16();
                    let body_read = response.take(4097).read_to_end(&mut bytes);
                    trace.body_bytes(bytes.len());
                    body_read.map_err(|_| Error("response_read_failed"))?;
                    return Err(enrollment_error(status, &bytes));
                }
                if !response.status().is_success() {
                    if u.host_str() == Some("cloud-api.phala.com") {
                        let status = response.status().as_u16();
                        let request_id = response
                            .headers()
                            .get("x-request-id")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned);
                        let mut bytes = Vec::new();
                        // A body-read error must not turn a known 422 into a timeout.
                        let read = response
                            .take(crate::phala_error::MAX_BODY as u64 + 1)
                            .read_to_end(&mut bytes);
                        trace.body_bytes(bytes.len());
                        if read.is_err() {
                            bytes.clear();
                        }
                        return Err(crate::phala_error::failure(
                            status,
                            &bytes,
                            request_id.as_deref(),
                            &r,
                        ));
                    }
                    if worker_update_endpoint(&u, &r.method) {
                        let status = response.status().as_u16();
                        let mut bytes = Vec::new();
                        let body_read = response.take(4097).read_to_end(&mut bytes);
                        trace.body_bytes(bytes.len());
                        body_read.map_err(|_| Error("response_read_failed"))?;
                        return Err(worker_update_error(status, &bytes));
                    }
                    if u.host_str() == Some("subnet.everyframe.studio")
                        && r.method == "GET"
                        && ["/v1/miner/jobs", "/mainnet/v1/miner/jobs"].contains(&u.path())
                    {
                        let status = response.status().as_u16();
                        let mut bytes = Vec::new();
                        let body_read = response.take(4097).read_to_end(&mut bytes);
                        trace.body_bytes(bytes.len());
                        body_read.map_err(|_| Error("response_read_failed"))?;
                        return Err(history_error(status, &bytes));
                    }
                    if u.host_str() == Some("subnet.everyframe.studio")
                        && r.method == "POST"
                        && [
                            "/v1/miner/automatic-offers-policy",
                            "/mainnet/v1/miner/automatic-offers-policy",
                            "/v1/miner/automatic-offers-preview",
                            "/mainnet/v1/miner/automatic-offers-preview",
                            "/v1/miner/offer-batch",
                            "/mainnet/v1/miner/offer-batch",
                            "/v1/miner/offer",
                            "/mainnet/v1/miner/offer",
                        ]
                        .contains(&u.path())
                    {
                        let status = response.status().as_u16();
                        let mut bytes = Vec::new();
                        let body_read = response.take(4097).read_to_end(&mut bytes);
                        trace.body_bytes(bytes.len());
                        body_read.map_err(|_| Error("response_read_failed"))?;
                        return Err(batch_offer_error(status, &bytes));
                    }
                    let host = u.host_str().unwrap_or_default();
                    if matches!(
                        host,
                        "api.vivix.ai"
                            | "queue.fal.run"
                            | "api-singapore.klingai.com"
                            | "api.dev.pika.art"
                    ) || crate::asian_provider::api_host(host)
                        || crate::submission::PROVIDER_ERROR_HOSTS.contains(&host)
                        || host.starts_with("api.") && host.ends_with(".bfl.ai")
                    {
                        let status = response.status().as_u16();
                        let mut bytes = Vec::new();
                        let body_read = response.take(64001).read_to_end(&mut bytes);
                        trace.body_bytes(bytes.len());
                        body_read.map_err(|_| Error("response_read_failed"))?;
                        let error = match host {
                            "openrouter.ai"
                                if status == 422
                                    && r.body
                                        .as_ref()
                                        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
                                        .is_some_and(|v| {
                                            v["model"].as_str().is_some_and(|id| {
                                                id == "minimax/minimax-h3-image"
                                                    || id == "minimax/minimax-h3-image-highspeed"
                                            })
                                        }) =>
                            {
                                crate::h3_image::http_error(status)
                            }
                            "openrouter.ai" => crate::provider_budget::http_error(status, &bytes),
                            "api.minimax.io"
                                if ["/v1/images/generations", "/v1/images/edits"]
                                    .contains(&u.path()) =>
                            {
                                crate::h3_image::http_error(status)
                            }
                            "api.wavespeed.ai" => crate::wavespeed::http_error(status, &bytes),
                            "queue.fal.run" => crate::submission::fal_http_error(status, &bytes),
                            "api-singapore.klingai.com" => {
                                crate::submission::kling_http_error(status, &bytes)
                            }
                            "api.dev.pika.art" => {
                                crate::submission::pika_http_error(status, &bytes)
                            }
                            "api.vivix.ai" => crate::submission::vivix_http_error(status, &bytes),
                            h if crate::asian_provider::api_host(h) => {
                                crate::asian_provider::http_error(status, &bytes)
                            }
                            _ => crate::diagnostics::http_error(status),
                        };
                        let secrets: Vec<String> = r
                            .headers
                            .iter()
                            .filter(|(key, _)| {
                                matches!(
                                    key.to_ascii_lowercase().as_str(),
                                    "authorization"
                                        | "x-key"
                                        | "x-api-key"
                                        | "x-goog-api-key"
                                        | "xi-api-key"
                                        | "api-key"
                                )
                            })
                            .flat_map(|(_, v)| {
                                [
                                    v.clone(),
                                    v.split_once(' ').map_or(v.as_str(), |(_, s)| s).to_owned(),
                                ]
                            })
                            .collect();
                        return Err(crate::submission::with_body_provider_error(
                            error, status, host, &bytes, &secrets,
                        ));
                    }
                    return Err(crate::diagnostics::http_error(response.status().as_u16()));
                }
                need(
                    response
                        .content_length()
                        .is_none_or(|len| len <= r.limit as u64),
                    "response_too_large",
                )?;
                let mut bytes = Vec::new();
                let body_read = response.take(r.limit as u64 + 1).read_to_end(&mut bytes);
                trace.body_bytes(bytes.len());
                body_read.map_err(|_| Error("response_read_failed"))?;
                need(bytes.len() <= r.limit, "response_too_large")?;
                return Ok(bytes);
            }
            Err(Error("unapproved_redirect"))
        })();
        trace.finish(&result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::enrollment_error;
    #[test]
    fn account_reads_are_concurrent_bounded_and_keep_result_order() {
        use std::sync::{
            Barrier,
            atomic::{AtomicUsize, Ordering},
        };
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let first_batch = Barrier::new(8);
        let requests = (0..17)
            .map(|i| super::Request::get(format!("https://example.com/{i}"), &["example.com"]))
            .collect();
        let results = super::parallel_reads(requests, |r| {
            let i: usize = r.url.rsplit('/').next().unwrap().parse().unwrap();
            let n = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            if i < 8 {
                first_batch.wait();
            }
            active.fetch_sub(1, Ordering::SeqCst);
            if i == 3 {
                Err(crate::Error("request_timeout"))
            } else {
                Ok(serde_json::json!(i))
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 8);
        assert_eq!(results.len(), 17);
        for (i, result) in results.into_iter().enumerate() {
            if i == 3 {
                assert_eq!(result.unwrap_err().0, "request_timeout");
            } else {
                assert_eq!(result.unwrap(), serde_json::json!(i));
            }
        }
    }
    #[test]
    fn account_read_batches_reject_mutations_before_transport() {
        let post = super::Request::get("https://example.com/", &["example.com"])
            .json("POST", &serde_json::json!({}));
        let mut body = super::Request::get("https://example.com/", &["example.com"]);
        body.body = Some(vec![]);
        let results =
            super::parallel_reads(vec![post, body], |_| panic!("must not reach transport"));
        assert!(
            results
                .into_iter()
                .all(|r| r.unwrap_err().0 == "invalid_method")
        );
    }
    #[test]
    fn history_errors_never_echo_remote_bodies() {
        assert_eq!(
            super::history_error(
                400,
                br#"{"error":"invalid_history_cursor","secret":"never displayed"}"#
            )
            .0,
            "invalid_history_cursor"
        );
        assert_eq!(
            super::history_error(400, br#"{"error":"private-key"}"#).0,
            "http_400"
        );
        assert_eq!(
            super::history_error(500, br#"{"error":"invalid_history_cursor"}"#).0,
            "http_500"
        );
    }
    #[test]
    fn batch_errors_are_fixed_bounded_and_status_checked() {
        assert_eq!(
            super::batch_offer_error(
                409,
                br#"{"error":"offer_revision_conflict","secret":"never echoed"}"#
            )
            .0,
            "offer_revision_conflict"
        );
        assert_eq!(
            super::batch_offer_error(409, br#"{"error":"offer_pricing_changed"}"#).0,
            "offer_pricing_changed"
        );
        for body in [b"secret".as_slice(), br#"{"error":"private-provider-key"}"#] {
            assert_eq!(super::batch_offer_error(409, body).0, "http_409");
        }
        assert_eq!(
            super::batch_offer_error(500, br#"{"error":"offer_revision_conflict"}"#).0,
            "http_500"
        );
        assert_eq!(
            super::batch_offer_error(409, &vec![b' '; 4097]).0,
            "http_409"
        );
    }
    #[test]
    fn worker_update_errors_are_scoped_bounded_and_never_echo_remote_text() {
        for action in [
            "worker-update-prepare",
            "worker-rollback",
            "worker-auto-update-policy",
            "worker-auto-update-report",
            "worker-auto-drain",
            "worker-auto-resume",
        ] {
            let u = url::Url::parse(&format!(
                "https://subnet.everyframe.studio/mainnet/v1/miner/{action}"
            ))
            .unwrap();
            assert!(super::worker_update_endpoint(&u, "POST"));
            assert!(!super::worker_update_endpoint(&u, "GET"));
        }
        for url in [
            "https://api.phala.network/v1/miner/worker-rollback",
            "https://subnet.everyframe.studio/mainnet/v1/miner/worker-auto-drain/other",
        ] {
            assert!(!super::worker_update_endpoint(
                &url::Url::parse(url).unwrap(),
                "POST"
            ));
        }
        for code in [
            "worker_auto_fleet_update_busy",
            "worker_auto_update_lease_lost",
            "worker_auto_update_not_enabled",
            "worker_auto_update_bootstrap_required",
            "worker_auto_update_disable_first",
            "worker_auto_update_recovery_pending",
            "worker_recovery_unavailable",
            "worker_recovery_in_progress",
            "worker_recovery_not_idle",
            "worker_recovery_target_changed",
            "worker_update_target_changed",
        ] {
            let body =
                serde_json::json!({"error":code,"message":"never expose this secret"}).to_string();
            assert_eq!(super::worker_update_error(409, body.as_bytes()).0, code);
            assert_eq!(crate::diagnostics::safe_code(code), code);
            assert_eq!(
                super::worker_update_error(500, body.as_bytes()).0,
                "http_500"
            );
        }
        assert_eq!(
            super::worker_update_error(400, br#"{"error":"invalid_updater_capsule"}"#).0,
            "invalid_updater_capsule"
        );
        for body in [
            b"secret".as_slice(),
            br#"{"error":"private-key"}"#,
            &vec![b' '; 4097],
        ] {
            assert_eq!(super::worker_update_error(409, body).0, "http_409");
        }
    }
    #[test]
    fn enrollment_errors_never_echo_remote_text_or_secrets() {
        assert_eq!(
            enrollment_error(503, br#"{"error":"public_onboarding_unavailable"}"#).0,
            "public_onboarding_unavailable_no_vm_created"
        );
        for bytes in [
            b"not json".as_slice(),
            br#"{"error":"secret-from-server"}"#,
            br#"{"message":"secret-from-server"}"#,
        ] {
            assert_eq!(enrollment_error(502, bytes).0, "http_502");
        }
        assert_eq!(enrollment_error(504, &vec![b' '; 4097]).0, "http_504");
        for (status, code) in [
            (503, "enrollment_timeout"),
            (503, "chain_lookup_unavailable"),
            (429, "enrollment_busy"),
            (401, "enrollment_expired"),
            (401, "enrollment_domain_mismatch"),
            (401, "hotkey_ownership_required"),
            (409, "enrollment_replayed"),
            (403, "hotkey_not_registered"),
        ] {
            let body = serde_json::json!({"error":code,"private":"never displayed"}).to_string();
            assert_eq!(enrollment_error(status, body.as_bytes()).0, code);
            assert_eq!(enrollment_error(502, body.as_bytes()).0, "http_502");
        }
    }
}

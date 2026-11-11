//! Bounded per-job operational metadata. No raw prompts, bodies, headers or URLs.
use crate::network::Request;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::VecDeque, time::Instant};
use url::Url;
const MAX_REQUESTS: usize = 12;
thread_local! { static STATE:RefCell<Option<State>>=const {RefCell::new(None)}; }
struct State {
    start: Instant,
    phase: &'static str,
    input: Value,
    requests: VecDeque<Value>,
    total: u64,
    failed: u64,
    output: Option<Value>,
}
pub struct JobGuard;
impl Drop for JobGuard {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = None);
    }
}
pub fn begin(spec: &Value, enabled: bool) -> JobGuard {
    STATE.with(|s| {
        *s.borrow_mut() = enabled.then(|| State {
            start: Instant::now(),
            phase: "submission",
            input: input_summary(spec),
            requests: VecDeque::new(),
            total: 0,
            failed: 0,
            output: None,
        })
    });
    JobGuard
}
pub fn phase(value: &'static str) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.phase = value;
        }
    });
}
fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_millis().min(86_400_000) as u64
}
fn token(v: &str, max: usize) -> bool {
    !v.is_empty()
        && !v.contains("://")
        && v.len() <= max
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:+-".contains(&b))
}
fn opaque_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn host_text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.len() <= 253
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".:-[]".contains(&b))
    })
}
fn safe_mime(v: &str) -> Option<String> {
    let v = v.split(';').next()?.trim().to_ascii_lowercase();
    (v.contains('/') && token(&v, 80)).then_some(v)
}
fn field_number(v: &Value) -> Value {
    v.as_u64()
        .filter(|n| *n <= 9_000_000_000_000_000)
        .map_or(Value::Null, |n| json!(n))
}
fn input_summary(spec: &Value) -> Value {
    let input = &spec["input"];
    let p = if input["parameters"].is_object() {
        &input["parameters"]
    } else {
        input
    };
    let mut settings = json!({});
    for k in [
        "duration",
        "resolution",
        "aspect_ratio",
        "aspectRatio",
        "output_format",
        "size",
        "quality",
        "n",
        "fps",
    ] {
        let v = &p[k];
        if v.as_str().is_some_and(|s| token(s, 32)) || v.as_u64().is_some_and(|n| n <= 100000) {
            settings[k] = v.clone();
        }
    }
    let refs = input["references"].as_array().cloned().unwrap_or_default();
    let items:Vec<Value>=refs.iter().take(8).map(|r|json!({"role":r["role"].as_str().filter(|s|["image","first","last","keyframe","mask","markup","video","audio"].contains(s)).unwrap_or("unknown"),"mime":r["mime"].as_str().and_then(safe_mime),"bytes":field_number(&r["bytes"]),"width":field_number(&r["width"]),"height":field_number(&r["height"]),"durationMs":field_number(&r["durationMs"])})).collect();
    json!({"model":spec["model"],"promptCharacters":input["prompt"].as_str().map_or(0,|s|s.chars().count()),"referenceCount":refs.len(),"references":items,"settings":settings})
}
fn memory_field(path: &str, key: &str) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.lines().find_map(|l| {
        l.strip_prefix(key)?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    })
}
fn runtime() -> Value {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let free = unsafe {
        if libc::statvfs(c"/tmp".as_ptr(), stat.as_mut_ptr()) == 0 {
            let stat = stat.assume_init();
            u128::from(stat.f_bavail)
                .checked_mul(u128::from(stat.f_frsize))
                .and_then(|bytes| u64::try_from(bytes).ok())
        } else {
            None
        }
    };
    json!({"workerVersion":env!("CARGO_PKG_VERSION"),"buildId":option_env!("EVERYFRAME_WORKER_BUILD_ID").unwrap_or("development"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"cpuCount":std::thread::available_parallelism().map_or(1,|v|v.get()),"processMemoryBytes":memory_field("/proc/self/status","VmRSS:"),"memoryAvailableBytes":memory_field("/proc/meminfo","MemAvailable:"),"scratchFreeBytes":free})
}
pub fn output(bytes: &[u8], output_hash: &str) {
    let mime = crate::native_provider::mime(bytes).ok().or_else(|| {
        if bytes.starts_with(b"GIF8") {
            Some("image/gif")
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
            Some("audio/wav")
        } else {
            None
        }
    });
    let dimensions = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        Some((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        ))
    } else {
        None
    };
    STATE.with(|s|{if let Some(s)=s.borrow_mut().as_mut(){s.output=Some(json!({"bytes":bytes.len(),"sha256":output_hash,"mime":mime,"width":dimensions.map(|d|d.0),"height":dimensions.map(|d|d.1)}));}});
}
pub fn snapshot() -> Option<Value> {
    STATE.with(|s|s.borrow().as_ref().map(|s|json!({"version":1,"elapsedMs":elapsed(s.start),"input":s.input,"runtime":runtime(),"network":{"total":s.total,"failed":s.failed,"dropped":s.total.saturating_sub(s.requests.len() as u64),"requests":s.requests},"output":s.output})))
}

/// Reject request IDs that echo a credential carried by this request.
pub fn public_request_id<'a>(candidate: Option<&'a str>, request: &Request) -> Option<&'a str> {
    candidate.filter(|value| {
        opaque_id(value)
            && !request.headers.iter().any(|(name, secret)| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "authorization"
                        | "x-key"
                        | "x-api-key"
                        | "api-key"
                        | "x-goog-api-key"
                        | "xi-api-key"
                ) && [
                    secret.as_str(),
                    secret.split_whitespace().last().unwrap_or(""),
                ]
                .iter()
                .any(|s| s.len() >= 8 && value.contains(s))
            })
    })
}

pub struct HttpSpan {
    start: Instant,
    value: Option<Value>,
}
impl HttpSpan {
    pub fn new(r: &Request) -> Self {
        let phase = STATE.with(|s| s.borrow().as_ref().map(|s| s.phase));
        let value=phase.map(|phase|{
   let host=Url::parse(&r.url).ok().and_then(|u|u.host_str().map(str::to_owned)).filter(|s|token(s,253));
   json!({"phase":phase,"host":host,"method":if ["GET","POST","PUT","PATCH","DELETE","HEAD"].contains(&r.method.as_str()){r.method.as_str()}else{"OTHER"},"elapsedMs":0,"dnsMs":null,"headersMs":null,"status":null,"contentType":null,"contentLength":null,"responseBytes":0,"requestBytes":r.body.as_ref().map_or(0,|b|b.len()),"timeoutSeconds":r.timeout,"responseLimitBytes":r.limit,"requestId":null,"retryAfterSeconds":null,"errorCode":null,"responseSha256":null,"jsonError":null,"responseKind":null,"responseKeys":[],"outputCount":null})
  });
        Self {
            start: Instant::now(),
            value,
        }
    }
    pub fn dns(&mut self, ms: u64) {
        if let Some(v) = self.value.as_mut() {
            v["dnsMs"] = json!(ms.min(86_400_000));
        }
    }
    pub fn response(
        &mut self,
        status: u16,
        content_type: Option<&str>,
        length: Option<u64>,
        request_id: Option<&str>,
        retry: Option<&str>,
    ) {
        if let Some(v) = self.value.as_mut() {
            v["status"] = json!(status);
            v["headersMs"] = json!(elapsed(self.start));
            v["contentType"] = json!(content_type.and_then(safe_mime));
            v["contentLength"] = json!(length.filter(|n| *n <= 9_000_000_000_000_000));
            v["requestId"] = json!(request_id.filter(|s| opaque_id(s)));
            v["retryAfterSeconds"] = json!(
                retry
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| *n <= 86400)
            );
        }
    }
    pub fn body_bytes(&mut self, n: usize) {
        if let Some(v) = self.value.as_mut() {
            v["responseBytes"] = json!(n);
        }
    }
    pub fn finish(mut self, result: &crate::Result<Vec<u8>>) {
        if let Some(mut v) = self.value.take() {
            v["elapsedMs"] = json!(elapsed(self.start));
            match result {
                Ok(b) => {
                    v["responseBytes"] = json!(b.len());
                    if b.len() <= 1024 * 1024 {
                        v["responseSha256"] = json!(crate::protocol::sha(b));
                    }
                }
                Err(e) => {
                    v["errorCode"] = json!(if token(e.0, 80) {
                        e.0
                    } else {
                        "unknown_transport_error"
                    })
                }
            }
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.total += 1;
                    if result.is_err() {
                        s.failed += 1;
                    }
                    if s.requests.len() >= MAX_REQUESTS {
                        s.requests.remove(1);
                    }
                    s.requests.push_back(v);
                }
            });
        }
    }
}
pub fn json_result(result: &serde_json::Result<Value>) {
    STATE.with(|s| {
        if let Some(last) = s.borrow_mut().as_mut().and_then(|s| s.requests.back_mut()) {
            match result {
                Err(e) => {
                    last["jsonError"] = json!(match e.classify() {
                        serde_json::error::Category::Io => "io",
                        serde_json::error::Category::Syntax => "syntax",
                        serde_json::error::Category::Data => "data",
                        serde_json::error::Category::Eof => "eof",
                    })
                }
                Ok(v) => {
                    last["responseKind"] = json!(if v.is_object() {
                        "object"
                    } else if v.is_array() {
                        "array"
                    } else if v.is_string() {
                        "string"
                    } else if v.is_null() {
                        "null"
                    } else if v.is_boolean() {
                        "boolean"
                    } else {
                        "number"
                    });
                    last["responseKeys"] = json!(
                        [
                            "id", "data", "status", "outputs", "output", "error", "code",
                            "message", "model", "result", "choices", "usage"
                        ]
                        .into_iter()
                        .filter(|k| v.get(*k).is_some())
                        .collect::<Vec<_>>()
                    );
                    last["outputCount"] = json!(
                        v["outputs"]
                            .as_array()
                            .or_else(|| v["data"]["outputs"].as_array())
                            .map(Vec::len)
                            .filter(|n| *n <= 1000)
                    );
                }
            }
        }
    });
}
fn exact(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn number(v: &Value) -> bool {
    v.as_u64().is_some_and(|n| n <= 9_000_000_000_000_000)
}
fn optional_number(v: &Value) -> bool {
    v.is_null() || number(v)
}
fn text(v: &Value, max: usize) -> bool {
    v.as_str().is_some_and(|s| token(s, max))
}
fn hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn sanitize(v: &Value) -> Option<Value> {
    if !exact(
        v,
        &[
            "version",
            "elapsedMs",
            "input",
            "runtime",
            "network",
            "output",
        ],
    ) || v["version"] != 1
        || !number(&v["elapsedMs"])
    {
        return None;
    }
    let i = &v["input"];
    if !exact(
        i,
        &[
            "model",
            "promptCharacters",
            "referenceCount",
            "references",
            "settings",
        ],
    ) || !text(&i["model"], 256)
        || !number(&i["promptCharacters"])
        || !number(&i["referenceCount"])
    {
        return None;
    }
    let refs = i["references"].as_array()?;
    if refs.len() > 8 {
        return None;
    }
    for r in refs {
        if !exact(
            r,
            &["role", "mime", "bytes", "width", "height", "durationMs"],
        ) || !text(&r["role"], 20)
            || !(r["mime"].is_null() || text(&r["mime"], 80))
            || !["bytes", "width", "height", "durationMs"]
                .iter()
                .all(|k| optional_number(&r[*k]))
        {
            return None;
        }
    }
    let settings = i["settings"].as_object()?;
    if settings.len() > 9
        || !settings.iter().all(|(k, v)| {
            [
                "duration",
                "resolution",
                "aspect_ratio",
                "aspectRatio",
                "output_format",
                "size",
                "quality",
                "n",
                "fps",
            ]
            .contains(&k.as_str())
                && (text(v, 32) || v.as_u64().is_some_and(|n| n <= 100000))
        })
    {
        return None;
    }
    let r = &v["runtime"];
    if !exact(
        r,
        &[
            "workerVersion",
            "buildId",
            "os",
            "arch",
            "cpuCount",
            "processMemoryBytes",
            "memoryAvailableBytes",
            "scratchFreeBytes",
        ],
    ) || !["workerVersion", "buildId", "os", "arch"]
        .iter()
        .all(|k| text(&r[*k], 40))
        || ![
            "cpuCount",
            "processMemoryBytes",
            "memoryAvailableBytes",
            "scratchFreeBytes",
        ]
        .iter()
        .all(|k| optional_number(&r[*k]))
    {
        return None;
    }
    let n = &v["network"];
    if !exact(n, &["total", "failed", "dropped", "requests"])
        || !["total", "failed", "dropped"]
            .iter()
            .all(|k| number(&n[*k]))
    {
        return None;
    }
    let requests = n["requests"].as_array()?;
    if requests.len() > MAX_REQUESTS {
        return None;
    }
    for r in requests {
        if !exact(
            r,
            &[
                "phase",
                "host",
                "method",
                "elapsedMs",
                "dnsMs",
                "headersMs",
                "status",
                "contentType",
                "contentLength",
                "responseBytes",
                "requestBytes",
                "timeoutSeconds",
                "responseLimitBytes",
                "requestId",
                "retryAfterSeconds",
                "errorCode",
                "responseSha256",
                "jsonError",
                "responseKind",
                "responseKeys",
                "outputCount",
            ],
        ) {
            return None;
        }
        if ![
            "submission",
            "provider_poll",
            "output_retrieval",
            "output_upload",
        ]
        .contains(&r["phase"].as_str()?)
            || !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OTHER"]
                .contains(&r["method"].as_str()?)
        {
            return None;
        }
        if ![
            "elapsedMs",
            "dnsMs",
            "headersMs",
            "contentLength",
            "responseBytes",
            "requestBytes",
            "timeoutSeconds",
            "responseLimitBytes",
            "retryAfterSeconds",
            "outputCount",
        ]
        .iter()
        .all(|k| optional_number(&r[*k]))
        {
            return None;
        }
        if !(r["status"].is_null()
            || r["status"]
                .as_u64()
                .is_some_and(|n| (100..=599).contains(&n)))
            || !(r["host"].is_null() || host_text(&r["host"]))
            || !(r["contentType"].is_null() || text(&r["contentType"], 80))
            || !(r["requestId"].is_null() || r["requestId"].as_str().is_some_and(opaque_id))
            || !(r["errorCode"].is_null() || text(&r["errorCode"], 80))
            || !(r["responseSha256"].is_null() || hash(&r["responseSha256"]))
        {
            return None;
        }
        if !(r["jsonError"].is_null()
            || ["io", "syntax", "data", "eof"].contains(&r["jsonError"].as_str()?))
            || !(r["responseKind"].is_null()
                || ["object", "array", "string", "number", "boolean", "null"]
                    .contains(&r["responseKind"].as_str()?))
        {
            return None;
        }
        let keys = r["responseKeys"].as_array()?;
        if keys.len() > 12
            || !keys.iter().all(|k| {
                k.as_str().is_some_and(|k| {
                    [
                        "id", "data", "status", "outputs", "output", "error", "code", "message",
                        "model", "result", "choices", "usage",
                    ]
                    .contains(&k)
                })
            })
        {
            return None;
        }
    }
    if !v["output"].is_null() {
        let o = &v["output"];
        if !exact(o, &["bytes", "sha256", "mime", "width", "height"])
            || !number(&o["bytes"])
            || !hash(&o["sha256"])
            || !(o["mime"].is_null() || text(&o["mime"], 80))
            || !optional_number(&o["width"])
            || !optional_number(&o["height"])
        {
            return None;
        }
    }
    if v.to_string().len() > 24000 {
        return None;
    }
    Some(v.clone())
}

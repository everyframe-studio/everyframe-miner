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
    fn json(&self, r: Request) -> Result<Value> {
        serde_json::from_slice(&self.bytes(r)?).map_err(|_| Error("invalid_remote_json"))
    }
}
pub struct PublicHttp;
pub fn safe_url(input: &str, hosts: &[String]) -> Result<Url> {
    let u = Url::parse(input).map_err(|_| Error("unapproved_destination"))?;
    let host = u.host_str().unwrap_or("");
    need(
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port_or_known_default() == Some(443)
            && u.fragment().is_none()
            && hosts.iter().any(|h| {
                if let Some(suffix) = h.strip_prefix("*.") {
                    host.ends_with(&format!(".{suffix}")) && host != suffix
                } else {
                    host == h
                }
            }),
        "unapproved_destination",
    )?;
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
    fn bytes(&self, mut r: Request) -> Result<Vec<u8>> {
        for hop in 0..=2 {
            let u = safe_url(&r.url, &r.hosts)?;
            let host = u.host_str().ok_or(Error("unapproved_destination"))?;
            let addresses = (host, 443)
                .to_socket_addrs()
                .map_err(|_| Error("dns_failed"))?
                .collect::<Vec<_>>();
            need(
                !addresses.is_empty() && addresses.iter().all(|a| public_address(a.ip())),
                "private_destination",
            )?;
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
            let response = request.send().map_err(|_| Error("remote_request_failed"))?;
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
                safe_url(target.as_str(), &r.redirect_hosts)?;
                r.url = target.to_string();
                r.hosts = r.redirect_hosts.clone();
                r.headers.clear();
                r.body = None;
                continue;
            }
            need(response.status().is_success(), "remote_request_failed")?;
            need(
                response
                    .content_length()
                    .is_none_or(|len| len <= r.limit as u64),
                "response_too_large",
            )?;
            let mut bytes = Vec::new();
            response
                .take(r.limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error("remote_request_failed"))?;
            need(bytes.len() <= r.limit, "response_too_large")?;
            return Ok(bytes);
        }
        Err(Error("unapproved_redirect"))
    }
}

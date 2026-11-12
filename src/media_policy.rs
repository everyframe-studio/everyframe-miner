//! Coordinator-authorized output destinations. Callers must first verify the
//! session/nonce-bound coordinator signature; these rules never authorize APIs.
use crate::{
    Result,
    invitation::PROVIDERS,
    n, need,
    protocol::{digest, exact},
    s,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "everyframe.provider-media-policy.v1";
pub const MAX_TTL_MS: i64 = 3_600_000;
#[derive(Default)]
pub struct Cache {
    revision: i64,
    fingerprint: String,
    issued_at: i64,
    expires_at: i64,
    providers: BTreeMap<String, Vec<String>>,
}
pub fn valid_host(rule: &str) -> bool {
    let host = rule.strip_prefix("*.").unwrap_or(rule);
    host.len() <= 253
        && host == host.to_ascii_lowercase()
        && host.split('.').count() >= 2
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && host
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_lowercase()))
        && host.parse::<std::net::IpAddr>().is_err()
        && ![
            "localhost",
            "local",
            "internal",
            "home.arpa",
            "co.uk",
            "com.cn",
            "com.au",
            "co.jp",
        ]
        .contains(&host)
        && !host.ends_with(".localhost")
        && !host.ends_with(".internal")
        && !host.ends_with(".local")
}
impl Cache {
    pub fn install(&mut self, policy: &Value, chain: &Value, now: i64) -> Result<()> {
        exact(
            policy,
            &[
                "schema",
                "chain",
                "revision",
                "issuedAt",
                "expiresAt",
                "providers",
            ],
        )?;
        need(
            policy["schema"] == SCHEMA && &policy["chain"] == chain,
            "invalid_media_policy",
        )?;
        let revision = n(&policy["revision"])?;
        let issued = n(&policy["issuedAt"])?;
        let expires = n(&policy["expiresAt"])?;
        need(
            revision > 0
                && revision >= self.revision
                && issued >= self.issued_at
                && issued <= now + 30_000
                && issued >= now - MAX_TTL_MS
                && expires > now
                && expires > issued
                && expires - issued <= MAX_TTL_MS,
            "stale_media_policy",
        )?;
        let entries = policy["providers"]
            .as_object()
            .ok_or(crate::Error("invalid_media_policy"))?;
        need(entries.len() <= PROVIDERS.len(), "invalid_media_policy")?;
        let mut providers = BTreeMap::new();
        for (provider, rules) in entries {
            need(
                PROVIDERS.iter().any(|(p, _)| p == provider),
                "invalid_media_policy",
            )?;
            let rules = rules
                .as_array()
                .ok_or(crate::Error("invalid_media_policy"))?;
            need(rules.len() <= 64, "invalid_media_policy")?;
            let mut hosts = BTreeSet::new();
            for rule in rules {
                let host = s(rule)?;
                need(
                    valid_host(host) && hosts.insert(host.to_string()),
                    "invalid_media_policy",
                )?;
            }
            providers.insert(provider.clone(), hosts.into_iter().collect());
        }
        let fingerprint = digest(
            &serde_json::to_value(&providers).map_err(|_| crate::Error("invalid_media_policy"))?,
        )?;
        need(
            revision != self.revision || fingerprint == self.fingerprint,
            "media_policy_revision_conflict",
        )?;
        self.revision = revision;
        self.fingerprint = fingerprint;
        self.issued_at = issued;
        self.expires_at = expires;
        self.providers = providers;
        Ok(())
    }
    pub fn active(&self, provider: &str, now: i64) -> bool {
        now < self.expires_at
            && now >= self.issued_at - 30_000
            && self.providers.contains_key(provider)
    }
    pub fn delivery_url(
        &self,
        provider: &str,
        raw: &str,
        baseline: Result<url::Url>,
        now: i64,
    ) -> Result<url::Url> {
        if now < self.expires_at
            && now >= self.issued_at - 30_000
            && let Some(hosts) = self.providers.get(provider)
        {
            return crate::network::safe_url(raw, hosts);
        }
        baseline
    }
    pub fn hosts(&self, provider: &str, baseline: &[&str], now: i64) -> Vec<String> {
        if now < self.expires_at
            && now >= self.issued_at - 30_000
            && let Some(hosts) = self.providers.get(provider)
        {
            return hosts.clone();
        }
        baseline.iter().map(|h| h.to_string()).collect()
    }
}

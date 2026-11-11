//! Reviewed LTX 2.5 request constraints and resumable native/Fal jobs.
use crate::{
    Error, Result, native_media, native_provider, need,
    protocol::{digest, sha},
    providers::{Output, Router},
    s,
};
use serde_json::{Value, json};

pub fn maximum(p: &Value, m: &Value) -> u64 {
    if m["ltxVariant"] != "fast" {
        return 10;
    }
    let low = if m["provider"] == "ltx" {
        p["resolution"].as_str().is_some_and(|r| {
            [
                "1280x720",
                "720x1280",
                "720x720",
                "1080x720",
                "720x1080",
                "1920x1080",
                "1080x1920",
                "1080x1080",
                "1620x1080",
                "1080x1620",
            ]
            .contains(&r)
        })
    } else {
        p["resolution"].is_null() || p["resolution"] == "720p" || p["resolution"] == "1080p"
    };
    if low && (m["ltxTask"] == "audio-to-video" || p["fps"].as_u64().unwrap_or(25) <= 25) {
        20
    } else {
        10
    }
}
pub fn check(p: &Value, spec: &Value, m: &Value) -> Result<()> {
    let refs = spec["input"]["references"]
        .as_array()
        .ok_or(Error("invalid_native_media_references"))?;
    let roles = m["referenceRoles"].as_object().unwrap();
    for (path, allowed) in roles {
        if let Some(value) = p.pointer(path).filter(|v| !v.is_null()) {
            let i = s(value)?
                .strip_prefix("@reference:")
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or(Error("unverified_native_reference"))?;
            let r = refs.get(i).ok_or(Error("invalid_native_reference_slot"))?;
            need(
                allowed == &r["role"] || allowed.as_array().is_some_and(|a| a.contains(&r["role"])),
                "invalid_reference_role",
            )?;
        }
    }
    need(
        refs.len() <= m["nativeCapabilities"]["maxReferences"].as_u64().unwrap() as usize,
        "invalid_native_media_references",
    )?;
    for r in refs {
        let audio = r["role"] == "audio";
        if m["provider"] == "ltx" {
            let limit = if audio { 32 } else { 15 };
            need(
                r["bytes"]
                    .as_u64()
                    .is_some_and(|n| n <= limit * 1024 * 1024),
                "invalid_reference_size",
            )?;
            if audio {
                need(
                    ["audio/wav", "audio/mpeg", "audio/mp4", "audio/ogg"].contains(&s(&r["mime"])?),
                    "invalid_reference_type",
                )?;
            }
        }
        if audio {
            need(
                r["durationMs"]
                    .as_u64()
                    .is_some_and(|n| n >= 2000 && n <= maximum(p, m) * 1000),
                "invalid_reference_duration",
            )?;
        }
    }
    if m["provider"] == "wavespeed" {
        return Ok(());
    }
    if m["ltxTask"] == "audio-to-video" {
        need(
            refs.iter().filter(|r| r["role"] == "audio").count() == 1,
            "invalid_native_media_references",
        )?;
        if m["provider"] == "ltx" && p.get("last_frame_uri").is_some() {
            need(
                p.get("image_uri").is_some(),
                "invalid_native_media_references",
            )?;
        }
    } else {
        let duration = &p["duration"];
        let auto = duration.is_null() || duration == "auto";
        need(
            auto || duration
                .as_u64()
                .is_some_and(|d| d >= 6 && d <= maximum(p, m) && d % 2 == 0),
            "invalid_native_video_duration",
        )?;
        if m["provider"] == "ltx" && auto {
            need(
                p.get("last_frame_uri").is_none(),
                "invalid_native_video_duration",
            )?;
        }
    }
    Ok(())
}
impl Router<'_> {
    pub(crate) fn ltx25_submit(&self, spec: &Value, m: &Value, body: &Value) -> Result<String> {
        let data = self
            .http
            .json(self.native_request(m, s(&m["apiPath"])?, Some(body))?)?;
        if !data["error"].is_null() {
            return Err(self.provider_failure(
                Error("provider_rejected"),
                s(&m["provider"])?,
                &data,
            ));
        }
        let id = &data[if m["provider"] == "fal" {
            "request_id"
        } else {
            "id"
        }];
        native_media::reference(spec, s(id)?)
    }
    fn ltx25_read(&self, reference: &str, spec: &Value, m: &Value) -> Result<Value> {
        let id = native_media::reference_id(spec, reference)?;
        if let Some((r, at, data)) = self.ltx25_cache.borrow().as_ref()
            && r == reference
            && (crate::now() - at < 5000
                || data["status"] == "completed"
                || data["status"] == "COMPLETED")
        {
            return Ok(data.clone());
        }
        let path = if m["provider"] == "fal" {
            format!("{}/requests/{id}/status", s(&m["apiPath"])?)
        } else {
            format!("{}/{id}", s(&m["apiPath"])?)
        };
        let data = self.http.json(self.native_request(m, &path, None)?)?;
        need(
            data[if m["provider"] == "fal" {
                "request_id"
            } else {
                "id"
            }] == id,
            "provider_id_mismatch",
        )?;
        self.ltx25_cache
            .replace(Some((reference.into(), crate::now(), data.clone())));
        Ok(data)
    }
    pub(crate) fn ltx25_poll(&self, reference: &str, spec: &Value, m: &Value) -> Result<String> {
        let d = self.ltx25_read(reference, spec, m)?;
        match d["status"].as_str() {
            Some("pending" | "IN_QUEUE") => Ok("IN_QUEUE".into()),
            Some("processing" | "IN_PROGRESS") => Ok("IN_PROGRESS".into()),
            Some("completed" | "COMPLETED") => Ok("COMPLETED".into()),
            Some("failed" | "FAILED") => Err(self.provider_failure(
                Error("provider_generation_failed"),
                s(&m["provider"])?,
                &d,
            )),
            _ => Err(Error("invalid_provider_status")),
        }
    }
    pub(crate) fn ltx25_result(&self, reference: &str, spec: &Value, m: &Value) -> Result<Output> {
        need(
            self.ltx25_poll(reference, spec, m)? == "COMPLETED",
            "provider_not_complete",
        )?;
        let data = if m["provider"] == "fal" {
            let id = native_media::reference_id(spec, reference)?;
            self.http.json(self.native_request(
                m,
                &format!("{}/requests/{id}", s(&m["apiPath"])?),
                None,
            )?)?
        } else {
            self.ltx25_read(reference, spec, m)?
        };
        let (url, hosts) = if m["provider"] == "fal" {
            (
                s(&data["video"]["url"])?,
                vec![
                    "fal.media",
                    "v3.fal.media",
                    "v3b.fal.media",
                    "v3c.fal.media",
                    "v3d.fal.media",
                ],
            )
        } else {
            (
                s(&data["result"]["video_url"])?,
                vec!["storage.googleapis.com"],
            )
        };
        // Output downloads carry no provider credentials or caller-selected hosts.
        let bytes = self.download_bounded(url, &hosts, None, &[], 90 * 1024 * 1024)?;
        let media = native_provider::bundle(
            vec![native_provider::part("video", "video/mp4", &bytes)?],
            vec![],
            Value::Null,
        )?;
        let response_hash = digest(
            &json!({"inputHash":digest(spec)?,"outputHash":sha(&media),"reference":reference}),
        )?;
        Ok(Output {
            media,
            response_hash,
        })
    }
}

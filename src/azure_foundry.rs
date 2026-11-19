//! Microsoft-hosted MAI deployments belong to the operator's Foundry resource.
use crate::{
    Error, Result, matches, native_media,
    native_provider::{bundle, part},
    need,
    network::Request,
    protocol::{decode, digest, id, sha},
    providers::Router,
    s,
};
use serde_json::{Value, json};

pub(crate) const CREDENTIAL_KEY: &str = "AZURE_FOUNDRY_CREDENTIALS";
const MAX_PIXELS: u64 = 4_194_304;
const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
const MAX_REFERENCE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn configuration(raw: &str) -> Result<Value> {
    let code = "invalid_foundry_credentials";
    need(raw.len() <= 8000, code)?;
    let config: Value = serde_json::from_str(raw).map_err(|_| Error(code))?;
    let fields = config.as_object().ok_or(Error(code))?;
    let deployments = config["deployments"].as_object().ok_or(Error(code))?;
    need(
        fields.len() == 3
            && matches("[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", &config["resource"])
            && matches("[A-Za-z0-9_-]{16,256}", &config["apiKey"])
            && !deployments.is_empty()
            && deployments.iter().all(|(variant, name)| {
                ["standard", "flash"].contains(&variant.as_str())
                    && matches("[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", name)
            })
            && (config["deployments"]["standard"].is_null()
                || config["deployments"]["standard"] != config["deployments"]["flash"]),
        code,
    )?;
    Ok(config)
}

pub(crate) fn deployment_configured(credentials: &Value, model: &Value) -> bool {
    credentials[CREDENTIAL_KEY]
        .as_str()
        .and_then(|raw| configuration(raw).ok())
        .is_some_and(|config| {
            config["deployments"][s(&model["maiVariant"]).unwrap_or("")].is_string()
        })
}

pub(crate) fn check(p: &Value, model: &Value, refs: &[Value]) -> Result<()> {
    let fields = p.as_object().ok_or(Error("invalid_native_media_fields"))?;
    need(
        refs.len() <= 5
            && refs.iter().all(|r| {
                r["role"] == "image"
                    && matches!(r["mime"].as_str(), Some("image/png" | "image/jpeg"))
            }),
        "invalid_native_media_references",
    )?;
    need(
        refs.iter().filter_map(|r| r["bytes"].as_u64()).sum::<u64>() <= MAX_REFERENCE_BYTES,
        "invalid_reference_size",
    )?;
    need(
        if refs.is_empty() {
            fields
                .keys()
                .all(|k| ["width", "height", "background"].contains(&k.as_str()))
        } else {
            fields.is_empty()
        },
        "invalid_native_media_fields",
    )?;
    if refs.is_empty() {
        dimensions(p)?;
        need(
            p.get("background").is_none()
                || (p["background"] == "transparent"
                    && model["nativeCapabilities"]["transparent"] == true),
            "invalid_native_image_background",
        )?;
    }
    Ok(())
}

fn dimensions(p: &Value) -> Result<(u64, u64)> {
    let size = |key| {
        p.get(key)
            .map_or(Some(1024), Value::as_u64)
            .ok_or(Error("invalid_native_image_size"))
    };
    let width = size("width")?;
    let height = size("height")?;
    need(
        width >= 768
            && height >= 768
            && width
                .checked_mul(height)
                .is_some_and(|area| area <= MAX_PIXELS),
        "invalid_native_image_size",
    )?;
    Ok((width, height))
}

fn png_dimensions(bytes: &[u8]) -> Result<(u64, u64)> {
    need(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR") && bytes.len() >= 33,
        "invalid_provider_media",
    )?;
    Ok((
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as u64,
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as u64,
    ))
}

impl Router<'_> {
    pub(crate) fn foundry_submit(&mut self, spec: &Value, model: &Value) -> Result<String> {
        let input = &spec["input"];
        let prompt = s(&input["prompt"])?;
        let params = &input["parameters"];
        let refs = input["references"]
            .as_array()
            .ok_or(Error("invalid_native_media_references"))?;
        check(params, model, refs)?;
        let config = configuration(s(&self.credentials[CREDENTIAL_KEY])?)?;
        let deployment = config["deployments"][s(&model["maiVariant"])?]
            .as_str()
            .ok_or(Error("foundry_deployment_required"))?;
        let host = format!("{}.services.ai.azure.com", s(&config["resource"])?);
        let endpoint = if refs.is_empty() {
            "generations"
        } else {
            "edits"
        };
        let mut request =
            Request::get(format!("https://{host}/mai/v1/images/{endpoint}"), &[&host]);
        request
            .headers
            .push(("api-key".into(), s(&config["apiKey"])?.into()));
        request.limit = RESPONSE_LIMIT;
        request.timeout = 600;
        let expected = if refs.is_empty() {
            let (width, height) = dimensions(params)?;
            let mut body =
                json!({"model":deployment,"prompt":prompt,"width":width,"height":height});
            if let Some(background) = params.get("background") {
                body["background"] = background.clone();
            }
            request = request.json("POST", &body);
            Some((width, height))
        } else {
            let boundary = format!("everyframe-{}", id());
            let mut body = Vec::new();
            for (name, value) in [("model", deployment), ("prompt", prompt)] {
                body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
            }
            for (index, reference) in refs.iter().enumerate() {
                let mime = s(&reference["mime"])?;
                let ext = if mime == "image/png" { "png" } else { "jpg" };
                let bytes = self.native_reference_bytes(reference)?;
                body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"reference-{index}.{ext}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
                body.extend_from_slice(&bytes);
                body.extend_from_slice(b"\r\n");
            }
            body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            request.method = "POST".into();
            request.headers.push((
                "content-type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            ));
            request.body = Some(body);
            None
        };
        self.guard_submission(&mut request)?;
        self.speech = None;
        let response = self.http.json(request)?;
        let data = response["data"]
            .as_array()
            .filter(|rows| rows.len() == 1)
            .ok_or(Error("invalid_provider_media"))?;
        let bytes = decode(
            data[0]["b64_json"]
                .as_str()
                .ok_or(Error("invalid_provider_media"))?,
        )?;
        let (width, height) = png_dimensions(&bytes)?;
        need(
            width > 0
                && height > 0
                && width * height <= MAX_PIXELS
                && expected.is_none_or(|size| size == (width, height)),
            "invalid_provider_media",
        )?;
        let media = bundle(
            vec![part("image", "image/png", &bytes)?],
            vec![],
            Value::Null,
        )?;
        let reference = native_media::reference(spec, &format!("sync_{}", id()))?;
        // The worker retains only this in-memory result. No provider file URL or second POST.
        self.speech = Some((reference.clone(), digest(spec)?, media, sha(&bytes)));
        Ok(reference)
    }
}

use everyframe_miner::{
    Result, models, native_media,
    network::{Http, Request},
    protocol::{self, Keys},
    providers::{Provider, Router},
    worker_catalog as catalog,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    os::unix::fs::PermissionsExt,
    process::Command,
};
const ID: &str = "studio/wavespeed/fixture/catalog-image-v2";
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/worker-catalog.json")).unwrap()
}
fn prepare(envelope: &Value, key: &str) -> Result<catalog::Prepared> {
    catalog::prepare(
        envelope,
        key,
        &json!({"network":"test"}),
        "catalog-miner",
        Some(("session", "nonce", 1000)),
    )
}
fn signed_mutation(f: &Value, mutate: impl FnOnce(&mut Value)) -> (Value, String) {
    let k = Keys::default();
    let mut v = f["envelope"]["value"].clone();
    mutate(&mut v);
    (protocol::signed(&v, &k.signing).unwrap(), k.signing_key())
}
fn definition(v: &mut Value, mutate: impl FnOnce(&mut Value)) {
    let mut d: Value =
        serde_json::from_str(v["entries"][0]["definition"].as_str().unwrap()).unwrap();
    mutate(&mut d);
    v["entries"][0]["definition"] = json!(d.to_string());
}
struct Api {
    calls: RefCell<Vec<Request>>,
}
impl Http for Api {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        self.calls.borrow_mut().push(r.clone());
        let url = url::Url::parse(&r.url).unwrap();
        if url.host_str() == Some("static.wavespeed.ai") {
            assert!(r.headers.is_empty());
            return Ok(b"\x89PNG\r\n\x1a\nfixture".to_vec());
        }
        assert_eq!(url.host_str(), Some("api.wavespeed.ai"));
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("Authorization") && v == "Bearer fixture-key")
        );
        let model = "fixture/catalog-image-v2";
        if r.method == "POST" {
            assert_eq!(url.path(), "/api/v3/fixture/catalog-image-v2");
            let b: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
            assert_eq!(b["prompt"], "Test dynamic catalog");
            return Ok(
                json!({"code":200,"data":{"id":"catalog_job","model":model,"status":"created"}})
                    .to_string()
                    .into_bytes(),
            );
        }
        assert_eq!(url.path(), "/api/v3/predictions/catalog_job/result");
        Ok(json!({"code":200,"data":{"id":"catalog_job","model":model,"status":"completed","outputs":["https://static.wavespeed.ai/catalog.png"]}}).to_string().into_bytes())
    }
}
struct Wire {
    keys: Keys,
    client_key: String,
    catalog: Value,
    clock: Cell<i64>,
    fail: Cell<bool>,
    calls: Cell<usize>,
}
impl everyframe_miner::worker::Runtime for Wire {
    fn now(&self) -> i64 {
        self.clock.get()
    }
    fn quote(&self, _: &[u8]) -> Result<Value> {
        Err(everyframe_miner::Error("unused"))
    }
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(path, "/v1/action");
        let req = protocol::verified(body, &self.client_key)?;
        if req["action"] == "claim" {
            assert_eq!(
                req["payload"]["models"],
                json!(["minimax/h3-max-turbo/text-to-video"])
            );
            assert!(
                req["payload"]["modelAvailability"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["model"] != "studio/wavespeed/fixture/catalog-image-v2")
            );
            return protocol::signed(
                &json!({"sessionId":req["sessionId"],"requestNonce":req["nonce"],"result":{"work":null}}),
                &self.keys.signing,
            );
        }
        assert_eq!(req["action"], "catalog_sync");
        assert!(req["payload"]["known"].as_array().unwrap().len() > 300);
        if self.fail.get() {
            return Err(everyframe_miner::Error("http_503"));
        }
        let mut manifest = self.catalog.clone();
        manifest["sessionId"] = req["sessionId"].clone();
        manifest["nonce"] = req["nonce"].clone();
        manifest["at"] = json!(self.clock.get());
        protocol::signed(
            &json!({"sessionId":req["sessionId"],"requestNonce":req["nonce"],"result":{"catalog":protocol::signed(&manifest,&self.keys.signing)?,"busy":true}}),
            &self.keys.signing,
        )
    }
}
struct LegacyCaps;
impl Provider for LegacyCaps {
    fn models(&self) -> Vec<String> {
        vec!["minimax/h3-max-turbo/text-to-video".into(), ID.into()]
    }
    fn refresh_availability(&mut self, now: i64) -> Option<Value> {
        Some(
            json!([{"model":"minimax/h3-max-turbo/text-to-video","status":"unchecked","checkedAt":now},{"model":ID,"status":"unchecked","checkedAt":now}]),
        )
    }
    fn submit(&mut self, _: &Value) -> Result<String> {
        unreachable!()
    }
    fn poll(&mut self, _: &str, _: &Value) -> Result<String> {
        unreachable!()
    }
    fn result(&mut self, _: &str, _: &Value) -> Result<everyframe_miner::providers::Output> {
        unreachable!()
    }
}
#[test]
fn signed_catalog_adoption_security_recovery_and_transport() {
    let f = fixture();
    let key = f["key"].as_str().unwrap();
    let envelope = &f["envelope"];
    // The JS publisher and measured Rust worker agree on every compiled contract,
    // including floats and non-ASCII keys. No provider/API is used in this test.
    assert_eq!(
        catalog::fingerprint(&f["numeric"]["value"]).unwrap(),
        f["numeric"]["hash"]
    );
    let known = catalog::known().unwrap();
    for k in known.as_array().unwrap() {
        assert!(
            f["baseline"].as_array().unwrap().contains(k),
            "baseline mismatch {}",
            k["model"]
        );
    }
    assert!(models::info(ID).is_err());
    let baseline = models::info("studio/wavespeed/google/nano-banana-2.1/text-to-image").unwrap();
    let hash = catalog::fingerprint(baseline).unwrap();
    assert!(prepare(envelope, &Keys::default().signing_key()).is_err());
    for (field, value) in [
        ("minerId", json!("other")),
        ("chain", json!({"network":"finney"})),
        ("nonce", json!("other")),
        ("sessionId", json!("other")),
        ("at", json!(1000000)),
        ("protocol", json!(2)),
    ] {
        let (e, k) = signed_mutation(&f, |v| v[field] = value);
        assert!(prepare(&e, &k).is_err(), "{field}");
    }
    for (field, value) in [
        ("provider", json!("bfl")),
        ("adapter", json!("evil")),
        ("referenceTransport", json!("inline-base64")),
        ("apiPath", json!("/api/v3/billing/delete")),
        ("apiModel", json!("../evil")),
        ("maxPrompt", json!(null)),
        ("nativeCapabilities", json!({})),
        ("requestHeaders", json!({"authorization":"evil"})),
        ("nativeSchema", json!(null)),
    ] {
        let (e, k) = signed_mutation(&f, |v| definition(v, |d| d[field] = value));
        assert!(prepare(&e, &k).is_err(), "{field}");
        assert!(models::info(ID).is_err());
    }
    let (e, k) = signed_mutation(&f, |v| {
        let entry = v["entries"][0].clone();
        v["entries"].as_array_mut().unwrap().push(entry);
    });
    assert!(prepare(&e, &k).is_err());
    let (e, k) = signed_mutation(&f, |v| v["entries"][0]["example"] = json!("{}"));
    assert!(prepare(&e, &k).is_err());
    let (e, k) = signed_mutation(&f, |v| {
        v["entries"][0]["definition"] = json!("x".repeat(128 * 1024 + 1))
    });
    assert!(prepare(&e, &k).is_err());
    let (extra_flag, flag_key) = signed_mutation(&f, |v| {
        let entry = v["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|e| e["model"] == "direct/google/catalog-image-v2")
            .unwrap();
        let mut d: Value = serde_json::from_str(entry["definition"].as_str().unwrap()).unwrap();
        d["catalogPolicy"]["unreviewedSwitch"] = json!(true);
        entry["definition"] = json!(d.to_string());
    });
    assert!(prepare(&extra_flag, &flag_key).is_err());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache").join("catalog.json");
    catalog::save_cache(&path, envelope).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // A second process verifies the signed cache and recovers without network or
    // the first process's global registry. Wrong scope/signature never installs.
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "catalog_cache_reader", "--nocapture"])
        .env("CATALOG_TEST_CACHE", &path)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        )
    );
    assert_eq!(
        catalog::install(prepare(envelope, key).unwrap()).unwrap(),
        3
    );
    assert_eq!(
        catalog::install(prepare(envelope, key).unwrap()).unwrap(),
        0
    );
    assert_eq!(catalog::fingerprint(baseline).unwrap(), hash); // in-flight references stay valid.
    let gid = "direct/google/catalog-image-v2";
    let gspec = models::spec(&json!({"model":gid,"prompt":"Catalog Google image"})).unwrap();
    assert_eq!(
        everyframe_miner::google::body(&gspec, models::info(gid).unwrap()).unwrap()["model"],
        "catalog-image-v2"
    );
    let fid = "studio/fal/fixture/catalog-video-v2/text-to-video";
    let fspec=models::spec(&json!({"model":fid,"prompt":"Catalog Fal video","seed":0,"duration":5,"resolution":"480p","aspect":"16:9","references":[],"promptExpansion":"balanced"})).unwrap();
    let (fal_path, body) = everyframe_miner::studio_video::body(&fspec, &[]).unwrap();
    assert_eq!(fal_path, "/fixture/catalog-video-v2/text-to-video");
    assert_eq!(body["prompt"], "Catalog Fal video");
    let current = models::info(ID).unwrap();
    assert_eq!(current["apiModel"], "fixture/catalog-image-v2");
    let api = Api {
        calls: RefCell::default(),
    };
    let mut router = Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-key"}));
    assert!(router.models().iter().any(|id| id == ID));
    let spec = models::spec(&json!({"model":ID,"prompt":"Test dynamic catalog"})).unwrap();
    let reference = router.submit(&spec).unwrap();
    let mut recovered = Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-key"}));
    assert_eq!(recovered.poll(&reference, &spec).unwrap(), "COMPLETED");
    assert!(
        !recovered
            .result(&reference, &spec)
            .unwrap()
            .media
            .is_empty()
    );
    assert_eq!(
        api.calls
            .borrow()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
    let (e, k) = signed_mutation(&f, |v| {
        definition(v, |d| d["apiModel"] = json!("different"))
    });
    assert!(prepare(&e, &k).is_err());
    assert_eq!(current, models::info(ID).unwrap());
    let (e, k) = signed_mutation(&f, |v| {
        v["entries"] = json!([]);
        v["models"].as_array_mut().unwrap().retain(|v| v != ID);
    });
    assert!(prepare(&e, &k).is_err());
    let (e, k) = signed_mutation(&f, |v| {
        v["models"].as_array_mut().unwrap().retain(|v| v != ID)
    });
    catalog::install(prepare(&e, &k).unwrap()).unwrap();
    assert!(!router.models().iter().any(|id| id == ID));
    assert_eq!(current, models::info(ID).unwrap()); // retained solely for recovery.
    // Exercise the authenticated runtime sync, periodic refresh, and outage
    // backoff. busy=true still supplies recovery definitions without submitting.
    let wire_keys = Keys::default();
    let client = Keys::default();
    let runtime = Wire {
        client_key: client.signing_key(),
        keys: wire_keys,
        catalog: f["envelope"]["value"].clone(),
        clock: Cell::new(1000),
        fail: Cell::new(false),
        calls: Cell::new(0),
    };
    let mut worker = everyframe_miner::worker::Worker {
        miner_id: "catalog-miner".into(),
        coordinator_key: runtime.keys.signing_key(),
        chain: json!({"network":"test"}),
        keys: client,
        session: json!({"sessionId":"wire-session"}),
        runtime,
        provider: Router::new(&api, json!({"WAVESPEED_API_KEY":"fixture-key"})),
        provider_pause: Default::default(),
    };
    assert!(catalog::refresh_to(&worker, &path).unwrap());
    assert!(!catalog::refresh_to(&worker, &path).unwrap());
    assert_eq!(worker.runtime.calls.get(), 1);
    worker.runtime.clock.set(301001);
    worker.runtime.fail.set(true);
    assert!(catalog::refresh_to(&worker, &path).is_err());
    assert!(!catalog::refresh_to(&worker, &path).unwrap());
    assert_eq!(worker.runtime.calls.get(), 2);
    assert!(catalog::visible(ID));
    worker.runtime.clock.set(361002);
    worker.runtime.fail.set(false);
    assert!(!catalog::refresh_to(&worker, &path).unwrap());
    assert_eq!(worker.runtime.calls.get(), 3);
    worker.session["sessionId"] = json!("wire-cache-failure");
    worker.runtime.catalog["models"]
        .as_array_mut()
        .unwrap()
        .retain(|id| id != ID);
    let unwritable = temp
        .path()
        .join("missing")
        .join("nested")
        .join("catalog.json");
    assert!(catalog::refresh_to(&worker, &unwritable).is_err());
    assert!(catalog::visible(ID));
    assert_eq!(current, models::info(ID).unwrap());
    let mut legacy = everyframe_miner::worker::Worker {
        miner_id: worker.miner_id,
        coordinator_key: worker.coordinator_key,
        chain: worker.chain,
        keys: worker.keys,
        session: json!({"sessionId":"legacy-session","expires":999999999}),
        runtime: worker.runtime,
        provider: LegacyCaps,
        provider_pause: Default::default(),
    };
    legacy.runtime.fail.set(false);
    assert!(legacy.claim().unwrap().is_null());
    std::fs::write(&path, b"bad cache").unwrap();
    assert!(catalog::restore(&path, key, &json!({"network":"test"}), "catalog-miner").is_err());
    assert_eq!(current, models::info(ID).unwrap());
    let link = temp.path().join("cache").join("symlink");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(catalog::save_cache(&link, envelope).is_err());
    assert!(catalog::restore(&link, key, &json!({"network":"test"}), "catalog-miner").is_err());
    // A model added by catalog must receive the same unary, complete-audio
    // normalization as a compiled Google TTS model (PRs 89 + 94 together).
    let template = "direct/google/gemini-3.8-flash-tts";
    let id = "direct/google/catalog-tts-v2";
    let mut definition = everyframe_miner::catalog_adapters::compiled(template).unwrap();
    definition["apiModel"] = json!("catalog-tts-v2");
    let mut example =
        models::spec(&json!({"model":template,"prompt":"Read this test sentence."})).unwrap();
    example["model"] = json!(id);
    let (tts, tts_key) = signed_mutation(&f, |v| {
        v["models"].as_array_mut().unwrap().push(json!(id));
        v["entries"].as_array_mut().unwrap().push(json!({"model":id,"template":template,"definition":definition.to_string(),"example":example.to_string()}));
    });
    assert_eq!(
        catalog::install(prepare(&tts, &tts_key).unwrap()).unwrap(),
        1
    );
    let spec = models::spec(&json!({"model":id,"prompt":"Read this test sentence."})).unwrap();
    let body = everyframe_miner::google::body(&spec, models::info(id).unwrap()).unwrap();
    assert_eq!(spec["input"]["background"], true);
    assert_eq!(body["background"], false);
    assert_eq!(body["store"], true);
    assert!(body["response_format"].get("delivery").is_none());
}
#[test]
fn catalog_cache_reader() {
    if let Ok(path) = std::env::var("CATALOG_TEST_CACHE") {
        let f = fixture();
        assert_eq!(
            catalog::restore(
                path.as_ref(),
                f["key"].as_str().unwrap(),
                &json!({"network":"test"}),
                "catalog-miner"
            )
            .unwrap(),
            3
        );
        assert!(native_media::info(ID).is_ok());
    }
}

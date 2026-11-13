use everyframe_miner::{
    Result, google, models,
    network::{Http, Request},
    protocol,
    providers::{Provider, Router, minimax_file_id},
    wav::normalize_openai_wav,
};
use serde_json::{Value, json};
use std::cell::RefCell;

#[test]
fn google_audio_omits_rejected_explicit_inline_without_changing_signed_input() {
    for model in [
        "direct/google/lyria-3.5",
        "direct/google/gemini-3.8-flash-tts",
        "direct/google/gemini-3.8-flash-lite-tts",
    ] {
        let spec =
            models::spec(&json!({"model":model,"prompt":"A cheerful musical greeting."})).unwrap();
        let before = protocol::digest(&spec).unwrap();
        let body = google::body(&spec, models::info(model).unwrap()).unwrap();
        assert!(body["response_format"].get("delivery").is_none());
        assert_eq!(body["background"], !model.ends_with("-tts"));
        assert_eq!(spec["input"]["background"], true);
        assert_eq!(before, protocol::digest(&spec).unwrap());
        let mut uri = spec.clone();
        uri["input"]["response_format"]["delivery"] = json!("uri");
        assert_eq!(
            google::body(&uri, models::info(model).unwrap()).unwrap()["response_format"]["delivery"],
            "uri"
        );
    }
}
#[test]
fn minimax_ids_compare_without_json_type_or_float_precision_errors() {
    let id = 1234567890123456789_u64;
    assert_eq!(
        minimax_file_id(&json!(id)).unwrap(),
        minimax_file_id(&json!(id.to_string())).unwrap()
    );
    for bad in [Value::Null, json!(-1), json!(1.5), json!("../x")] {
        assert!(minimax_file_id(&bad).is_err());
    }
}
#[test]
fn google_background_pcm_is_wrapped_as_requested_wav_with_bound_rate() {
    let spec =
        models::spec(&json!({"model":"direct/google/gemini-3.8-flash-tts","prompt":"Welcome."}))
            .unwrap();
    let m = google::output_model(
        &spec,
        models::info("direct/google/gemini-3.8-flash-tts").unwrap(),
    )
    .unwrap();
    let block = json!({"type":"audio","mime_type":"audio/l16","sample_rate":24000,"channels":1,"data":"AAAAAA=="});
    let response =
        json!({"status":"completed","steps":[{"type":"model_output","content":[block.clone()]}]});
    assert_eq!(google::media_blocks(&response, &m).unwrap().len(), 1);
    let bytes = vec![1, 2, 3, 4];
    let wav = google::normalize_media(&block, &m, bytes.clone()).unwrap();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[44..], bytes.as_slice());
    let mut wrong = block.clone();
    wrong["sample_rate"] = json!(16000);
    assert!(google::normalize_media(&wrong, &m, bytes.clone()).is_err());
    assert!(google::normalize_media(&block, &m, vec![1]).is_err());
    let wav = everyframe_miner::wav::pcm16_wav(vec![1, 2, 3, 4], 24000, 1).unwrap();
    assert_eq!(
        google::normalize_media(&block, &m, wav.clone()).unwrap(),
        wav
    );
    let mut partial = wav.clone();
    partial.extend([1, 2]);
    assert!(google::normalize_media(&block, &m, partial).is_err());
}
fn streaming_wav() -> Vec<u8> {
    let mut b = vec![0_u8; 44 + 480];
    b[0..4].copy_from_slice(b"RIFF");
    b[4..8].fill(255);
    b[8..16].copy_from_slice(b"WAVEfmt ");
    b[16..20].copy_from_slice(&16_u32.to_le_bytes());
    b[20..22].copy_from_slice(&1_u16.to_le_bytes());
    b[22..24].copy_from_slice(&1_u16.to_le_bytes());
    b[24..28].copy_from_slice(&24000_u32.to_le_bytes());
    b[28..32].copy_from_slice(&48000_u32.to_le_bytes());
    b[32..34].copy_from_slice(&2_u16.to_le_bytes());
    b[34..36].copy_from_slice(&16_u16.to_le_bytes());
    b[36..40].copy_from_slice(b"data");
    b[40..44].fill(255);
    b
}
#[test]
fn openai_unknown_length_wav_is_finalized_without_touching_samples() {
    let original = streaming_wav();
    let fixed = normalize_openai_wav(original.clone()).unwrap();
    assert_eq!(&fixed[44..], &original[44..]);
    assert_eq!(
        u32::from_le_bytes(fixed[4..8].try_into().unwrap()) as usize,
        fixed.len() - 8
    );
    assert_eq!(
        u32::from_le_bytes(fixed[40..44].try_into().unwrap()) as usize,
        fixed.len() - 44
    );
    assert_eq!(normalize_openai_wav(fixed.clone()).unwrap(), fixed);
    let mut odd = original.clone();
    odd.pop();
    assert!(normalize_openai_wav(odd).is_err());
    let mut bad = original.clone();
    bad[22..24].fill(0);
    assert!(normalize_openai_wav(bad).is_err());
    assert_eq!(
        normalize_openai_wav(b"other format".to_vec()).unwrap(),
        b"other format"
    );
}
struct MinimaxV2 {
    model: String,
    calls: RefCell<Vec<Request>>,
}
impl Http for MinimaxV2 {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        self.calls.borrow_mut().push(r.clone());
        if r.url.starts_with("https://cdn.hailuoai.com/") {
            assert!(r.headers.is_empty());
            return Ok(b"fixture-video".to_vec());
        }
        assert!(r.headers.iter().any(|(k, _)| k == "authorization"));
        let body = if r.method == "POST" {
            assert!(r.url.ends_with("/v2/video_generation"));
            json!({"task_id":"424010985738629"})
        } else {
            assert!(
                r.url
                    .ends_with("/v2/query/video_generation/424010985738629")
            );
            json!({"task":{"id":"424010985738629","model":self.model,"status":"succeeded","content":{"url":"https://cdn.hailuoai.com/test.mp4"}}})
        };
        Ok(body.to_string().into_bytes())
    }
}
#[test]
fn all_four_h3_routes_accept_official_v2_task_response_and_get_only_recovery() {
    for model in [
        "minimax-h3",
        "minimax-h3-max",
        "minimax-h3-highspeed",
        "minimax-h3-lite",
    ] {
        let model = format!("studio/native/minimax/{model}");
        let spec = models::spec(&json!({"model":model,"prompt":"A quiet mountain lake."})).unwrap();
        let api = MinimaxV2 {
            model: models::info(&model).unwrap()["apiModel"]
                .as_str()
                .unwrap()
                .into(),
            calls: RefCell::new(vec![]),
        };
        let mut router = Router::new(&api, json!({"MINIMAX_API_KEY":"fixture-secret"}));
        let reference = router.submit(&spec).unwrap();
        assert_eq!(router.poll(&reference, &spec).unwrap(), "COMPLETED");
        let output = router.result(&reference, &spec).unwrap();
        let bundle: Value = serde_json::from_slice(&output.media).unwrap();
        assert_eq!(bundle["parts"][0]["type"], "video");
        let mut recovery = Router::new(&api, json!({"MINIMAX_API_KEY":"fixture-secret"}));
        assert_eq!(recovery.poll(&reference, &spec).unwrap(), "COMPLETED");
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
    }
}

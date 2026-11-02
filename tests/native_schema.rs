use everyframe_miner::{native_schema, protocol};
use serde_json::json;
#[test]
fn reviewed_schemas_validate_full_settings_and_numeric_canonicalization() {
    let schema = json!({"type":"object","properties":{"size":{"enum":["hd","fhd"]},"guidance":{"type":"number","minimum":1.5,"maximum":10},"frames":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"object","properties":{"at":{"type":"number","minimum":0,"maximum":1},"image":{"type":"string","pattern":"^https://"}},"required":["at","image"],"additionalProperties":false}}},"required":["size","guidance"],"additionalProperties":false});
    let valid = json!({"size":"fhd","guidance":"7.25","frames":[{"at":"0.5","image":"https://api.everyframe.studio/asset"}]});
    protocol::digest(&valid).unwrap();
    native_schema::validate(&schema, &valid).unwrap();
    for bad in [
        json!({"size":"4k","guidance":5}),
        json!({"size":"hd","guidance":"NaN"}),
        json!({"size":"hd","guidance":12}),
        json!({"size":"hd","guidance":5,"callback":"https://evil.example"}),
    ] {
        assert!(native_schema::validate(&schema, &bad).is_err());
    }
}
#[test]
fn external_references_must_be_content_bound_and_account_endpoints_cannot_be_injected() {
    let v = json!({"prompt":"A poster for https://example.com","image":"@reference:0"});
    let mut used = vec![];
    let result = native_schema::resolve(
        &v,
        &["data:image/png;base64,AAAA".into()],
        "/prompt",
        "",
        &mut used,
    )
    .unwrap();
    assert_eq!(result["image"], "data:image/png;base64,AAAA");
    assert_eq!(used, vec![0]);
    for value in [
        json!({"image":"https://evil.example/image.png"}),
        json!({"image":"data:image/png;base64,AAAA"}),
        json!({"image":"@reference:1"}),
    ] {
        assert!(
            native_schema::resolve(&value, &["verified".into()], "/prompt", "", &mut vec![])
                .is_err()
        );
    }
}
#[test]
fn unresolved_refs_and_unbounded_schema_recursion_fail_closed() {
    assert!(
        native_schema::validate(&json!({"$ref":"https://evil.example/schema"}), &json!({}))
            .is_err()
    );
    let mut s = json!({"type":"integer"});
    let mut v = json!(1);
    for _ in 0..40 {
        s = json!({"type":"array","items":s});
        v = json!([v]);
    }
    assert!(native_schema::validate(&s, &v).is_err());
}

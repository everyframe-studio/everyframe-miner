use everyframe_miner::{cloud, invitation};
use serde_json::json;

#[test]
fn workspace_and_generation_keys_reach_only_an_explicitly_permitted_worker() {
    let invitation = json!({"minerId":"fixture-miner","request":{"compose_file":{
        "allowed_envs":["MINER_ID","MINER_TOKEN","DASHSCOPE_API_KEY","DASHSCOPE_WORKSPACE_ID","BYTEPLUS_API_KEY","VOLCENGINE_API_KEY"],
        "docker_compose_file":"services:\n  worker:\n    image: ghcr.io/everyframe-studio/everyframe-miner:fixture\n",
        "pre_launch_script":invitation::PUBLIC_SCRIPT}}});
    let credentials = json!({"MINER_TOKEN":"fixture-token","DASHSCOPE_API_KEY":"fixture-alibaba-key",
        "DASHSCOPE_WORKSPACE_ID":"workspace-123","BYTEPLUS_API_KEY":"fixture-byteplus-key",
        "VOLCENGINE_API_KEY":"fixture-volcengine-key","PHALA_CLOUD_API_KEY":"must-stay-local",
        "OPENROUTER_MANAGEMENT_KEY":"must-stay-local"});
    let active = cloud::environment(&invitation, &credentials, true).unwrap();
    assert_eq!(active["DASHSCOPE_WORKSPACE_ID"], "workspace-123");
    assert_eq!(active["BYTEPLUS_API_KEY"], "fixture-byteplus-key");
    assert_eq!(active["VOLCENGINE_API_KEY"], "fixture-volcengine-key");
    assert!(active.get("PHALA_CLOUD_API_KEY").is_none());
    assert!(active.get("OPENROUTER_MANAGEMENT_KEY").is_none());
    let withheld = cloud::environment(&invitation, &credentials, false).unwrap();
    assert_eq!(withheld["DASHSCOPE_API_KEY"], invitation::DISABLED);
    assert_eq!(withheld["DASHSCOPE_WORKSPACE_ID"], "");
    let mut invalid = credentials.clone();
    invalid["DASHSCOPE_WORKSPACE_ID"] = json!("https://attacker.example");
    assert_eq!(
        cloud::environment(&invitation, &invalid, true)
            .unwrap_err()
            .0,
        "alibaba_workspace_required"
    );
    let mut old = invitation;
    old["request"]["compose_file"]["allowed_envs"] =
        json!(["MINER_ID", "MINER_TOKEN", "BYTEPLUS_API_KEY"]);
    let narrow = cloud::environment(&old, &credentials, true).unwrap();
    assert!(narrow.get("DASHSCOPE_API_KEY").is_none());
    assert!(narrow.get("DASHSCOPE_WORKSPACE_ID").is_none());
    assert!(narrow.get("VOLCENGINE_API_KEY").is_none());
}

mod common;
use everyframe_miner::{
    Error, Result, cli, hotkey, invitation,
    network::{Http, Request},
    onboarding,
    state::State,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

const ADDRESS: &str = "5HQfvwpHm2njGiF716YwjdXgeeZoEiyFLZFvFAriZaiHNKc3";
struct Chain {
    mode: &'static str,
    calls: RefCell<Vec<Value>>,
}
impl Http for Chain {
    fn bytes(&self, r: Request) -> Result<Vec<u8>> {
        assert_eq!(r.method, "POST");
        assert!(r.headers.iter().all(|(k, _)| k != "authorization"));
        let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
        self.calls.borrow_mut().push(body.clone());
        let chain = hotkey::chain(
            &json!({"network":if r.url.contains("entrypoint-finney") {"mainnet"} else {"testnet"}}),
        )
        .unwrap();
        let result = match body["method"].as_str().unwrap() {
            "chain_getBlockHash" => {
                if self.mode == "genesis" {
                    json!("wrong")
                } else {
                    chain["genesis"].clone()
                }
            }
            "chain_getFinalizedHead" => json!(format!("0x{}", "a".repeat(64))),
            "state_getStorage" => {
                assert_eq!(body["params"][1], format!("0x{}", "a".repeat(64)));
                if self.calls.borrow().len() == 3 {
                    match self.mode {
                        "absent" => Value::Null,
                        "baduid" => json!("0x810000"),
                        _ => json!("0x8100"),
                    }
                } else if self.mode == "reverse" {
                    json!("0x00")
                } else {
                    json!(format!(
                        "0x{}",
                        hex::encode(hotkey::public_address(ADDRESS).unwrap())
                    ))
                }
            }
            _ => panic!("unexpected method"),
        };
        if self.mode == "rpcerror" {
            return Ok(json!({"jsonrpc":"2.0","id":1,"error":{"code":-1}})
                .to_string()
                .into_bytes());
        }
        Ok(json!({"jsonrpc":"2.0","id":1,"result":result})
            .to_string()
            .into_bytes())
    }
}
#[test]
fn registration_checks_genesis_finalized_forward_and_reverse_mapping() {
    assert_eq!(
        onboarding::uid_key(117, &hotkey::public_address(ADDRESS).unwrap()),
        "0x658faa385070e074c85bf6b568cf0555aab1b4e78e1ea8305462ee53b3686dc87500834ddab5c21c437ae0407b46ea62fb85ec67974b8f2b4ddea7476c0177ed3c46e0b427b408c8199e0be0bec25dbc3c60"
    );
    for network in ["mainnet", "testnet"] {
        let chain = Chain {
            mode: "ok",
            calls: RefCell::default(),
        };
        let out = onboarding::check_registration(&chain, network, ADDRESS).unwrap();
        assert_eq!(out["uid"], 129);
        assert_eq!(out["netuid"], if network == "mainnet" { 117 } else { 566 });
        assert_eq!(out["chainTransactionSubmitted"], false);
        assert_eq!(chain.calls.borrow().len(), 4);
    }
    for mode in ["genesis", "baduid", "reverse", "rpcerror"] {
        assert!(
            onboarding::check_registration(
                &Chain {
                    mode,
                    calls: RefCell::default()
                },
                "mainnet",
                ADDRESS
            )
            .is_err()
        );
    }
    let absent = onboarding::check_registration(
        &Chain {
            mode: "absent",
            calls: RefCell::default(),
        },
        "mainnet",
        ADDRESS,
    )
    .unwrap();
    assert_eq!(absent["registered"], false);
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    assert!(onboarding::save_registration(&state, &absent, None).is_err());
    assert!(!state.directory.exists());
    let invalid = Chain {
        mode: "ok",
        calls: RefCell::default(),
    };
    assert!(onboarding::check_registration(&invalid, "mainnet", "invalid").is_err());
    assert!(invalid.calls.borrow().is_empty());
}
#[test]
fn public_registration_never_changes_an_initialized_identity() {
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    let out = onboarding::check_registration(
        &Chain {
            mode: "ok",
            calls: RefCell::default(),
        },
        "mainnet",
        ADDRESS,
    )
    .unwrap();
    onboarding::save_registration(&state, &out, None).unwrap();
    assert_eq!(
        state.read("registration", false).unwrap()["hotkey"],
        ADDRESS
    );
    state
        .write(
            "config",
            &json!({"invitation":{"value":{"hotkey":"another","network":"mainnet"}}}),
        )
        .unwrap();
    assert!(onboarding::save_registration(&state, &out, None).is_err());
}
#[test]
fn key_management_is_atomic_private_and_preserves_auth_and_other_providers() {
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    state
        .write(
            "credentials",
            &json!({"CONSOLE_AUTH":{"secret":"test-delegate"},"MINER_TOKEN":"test-legacy-token"}),
        )
        .unwrap();
    for (provider, key) in invitation::PROVIDERS {
        assert_eq!(onboarding::credential_key(provider).unwrap(), key);
        let out = onboarding::save_keys(
            &state,
            &json!({key:format!("synthetic-{provider}-key")}),
            None,
        )
        .unwrap();
        assert!(!out.to_string().contains("synthetic"));
        assert_eq!(out["workerChanged"], false);
    }
    onboarding::save_keys(
        &state,
        &json!({"PHALA_CLOUD_API_KEY":"synthetic-phala-key"}),
        None,
    )
    .unwrap();
    let before = state.read("credentials", false).unwrap();
    assert_eq!(before.as_object().unwrap().len(), 12);
    assert_eq!(
        fs::metadata(state.path("credentials").unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        onboarding::save_keys(
            &state,
            &json!({"FAL_KEY":"valid-synthetic-key","MINIMAX_API_KEY":"short"}),
            None
        )
        .is_err()
    );
    assert_eq!(state.read("credentials", false).unwrap(), before);
    assert!(
        onboarding::save_keys(&state, &json!({"CONSOLE_AUTH":"overwrite-delegate"}), None).is_err()
    );
    onboarding::save_keys(&state, &json!({}), Some("fal")).unwrap();
    let after = state.read("credentials", false).unwrap();
    assert!(after.get("FAL_KEY").is_none());
    assert_eq!(after["CONSOLE_AUTH"], before["CONSOLE_AUTH"]);
    assert_eq!(after["MINIMAX_API_KEY"], before["MINIMAX_API_KEY"]);
    state
        .write("deployment", &json!({"phase":"activate_intent"}))
        .unwrap();
    assert!(onboarding::save_keys(&state, &json!({}), Some("minimax")).is_err());
    assert_eq!(state.read("credentials", false).unwrap(), after);
}
fn command(state: &State, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_everycli"));
    c.arg("miner")
        .args(args)
        .arg("--state-dir")
        .arg(&state.directory);
    c
}
#[test]
fn cli_accepts_piped_secret_without_echo_and_works_before_init() {
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    let mut c = command(
        &state,
        &["set-api-keys", "--provider", "minimax", "--stdin", "--json"],
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(b"synthetic-hidden-key\n")
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-hidden-key"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-hidden-key"));
    let providers = command(&state, &["providers", "--json"]).output().unwrap();
    assert!(providers.status.success());
    assert!(!String::from_utf8_lossy(&providers.stdout).contains("synthetic-hidden-key"));
    assert!(
        !command(&state, &["set-api-keys", "--provider", "fal", "--json"])
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = state.read("credentials", false).unwrap();
    assert!(
        !command(
            &state,
            &["remove-api-key", "--provider", "minimax", "--json"]
        )
        .stdin(Stdio::null())
        .output()
        .unwrap()
        .status
        .success()
    );
    assert_eq!(state.read("credentials", false).unwrap(), before);
    assert!(
        command(
            &state,
            &["remove-api-key", "--provider", "minimax", "--yes", "--json"]
        )
        .output()
        .unwrap()
        .status
        .success()
    );
    assert!(
        state
            .read("credentials", false)
            .unwrap()
            .get("MINIMAX_API_KEY")
            .is_none()
    );
}
#[test]
fn parser_rejects_key_values_on_command_line_and_wrong_option_scopes() {
    for args in [
        vec!["miner", "set-api-keys", "--minimax", "secret"],
        vec!["miner", "remove-api-key", "--stdin"],
        vec!["miner", "apply-api-keys", "--provider", "fal"],
    ] {
        assert!(cli::parse(&args.into_iter().map(str::to_string).collect::<Vec<_>>()).is_err());
    }
    assert_eq!(
        onboarding::credential_key("unknown"),
        Err(Error("unknown_provider"))
    );
    assert!(onboarding::wallet_path("../wallet", "default").is_err());
}

#[test]
fn interactive_prompt_hides_key_and_blank_input_preserves_it() {
    use std::{
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::process::CommandExt,
        },
        time::{Duration, Instant},
    };
    let tmp = common::tempdir();
    let state = State {
        directory: tmp.path().join("profile"),
    };
    for input in ["synthetic-terminal-key\n", "\n"] {
        let (mut master_fd, mut slave_fd) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master_fd,
                    &mut slave_fd,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let mut master = unsafe { fs::File::from_raw_fd(master_fd) };
        let slave = unsafe { fs::File::from_raw_fd(slave_fd) };
        let mut cmd = command(&state, &["set-api-keys", "--provider", "fal"]);
        cmd.stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave);
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        // Command owns copies of the slave fd; drop them so master eventually sees EOF.
        drop(cmd);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut sent = false;
        loop {
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("hidden prompt timed out");
            }
            let mut poll = libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 100) } > 0 {
                let mut buf = [0u8; 4096];
                match master.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => output.extend_from_slice(&buf[..n]),
                }
            }
            if !sent && String::from_utf8_lossy(&output).contains("Enter keeps existing): ") {
                // rpassword writes the prompt before switching echo off. Wait until
                // the terminal reports ECHO disabled, never race a secret into it.
                let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
                if unsafe { libc::tcgetattr(master.as_raw_fd(), term.as_mut_ptr()) } == 0
                    && unsafe { term.assume_init() }.c_lflag & libc::ECHO == 0
                {
                    master.write_all(input.as_bytes()).unwrap();
                    sent = true;
                }
            }
        }
        assert!(sent);
        assert!(child.wait().unwrap().success());
        assert!(!String::from_utf8_lossy(&output).contains("synthetic-terminal-key"));
        assert_eq!(
            state.read("credentials", false).unwrap()["FAL_KEY"],
            "synthetic-terminal-key"
        );
    }
}

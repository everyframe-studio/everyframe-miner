use super::{Runtime, Worker};
use crate::{Error, Result, media_policy, model_availability, network::Http, providers::Router};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

pub(super) const LIMIT: usize = 5;
struct Assignment {
    work: Value,
    session: Value,
    policy: media_policy::Cache,
    catalog: model_availability::CatalogCache,
}

#[derive(Default)]
struct CycleLog {
    previous: String,
    at: i64,
}
impl CycleLog {
    fn record(&mut self, outcome: Result<Value>) {
        let now = crate::now();
        let message = match &outcome {
            Ok(v) => v.to_string(),
            Err(error) => error.0.to_string(),
        };
        if message == self.previous && now - self.at < 60_000 {
            return;
        }
        match outcome {
            Ok(v) => println!("{}", json!({"event":"worker_cycle","result":v})),
            Err(e) => eprintln!("{}", json!({"event":"worker_cycle_failed","reason":e.0})),
        }
        self.previous = message;
        self.at = now;
    }
}

pub(super) fn run<R: Runtime + Clone + Send>(
    dispatcher: &mut Worker<R, Router<'_>>,
    http: &(impl Http + Sync),
    stop: &AtomicBool,
) -> Result<()> {
    let delivery = std::sync::Mutex::new(());
    thread::scope(|scope| {
        let mut senders = Vec::with_capacity(LIMIT);
        let mut handlers = Vec::with_capacity(LIMIT);
        let (done, finished) = mpsc::channel();
        for slot in 0..LIMIT {
            let (send, assignments) = mpsc::sync_channel::<Assignment>(1);
            senders.push(send);
            let done = done.clone();
            let runtime = dispatcher.runtime.clone();
            let miner_id = dispatcher.miner_id.clone();
            let coordinator_key = dispatcher.coordinator_key.clone();
            let keys = dispatcher.keys.clone();
            let chain = dispatcher.chain.clone();
            let credentials = dispatcher.provider.credentials.clone();
            let budget = dispatcher.provider.budget.clone();
            let delivery = &delivery;
            handlers.push(scope.spawn(move || {
                let mut provider = Router::new(http, credentials);
                provider.budget = budget;
                let mut worker = Worker {
                    miner_id,
                    coordinator_key,
                    keys,
                    chain,
                    provider,
                    runtime,
                    session: Value::Null,
                    provider_pause: HashMap::new(),
                };
                for assignment in assignments {
                    worker.session = assignment.session;
                    worker.provider.media_policy = assignment.policy;
                    worker.provider.catalog = assignment.catalog;
                    let outcome = worker.execute_work(assignment.work, Some(delivery));
                    if outcome.as_ref().is_ok_and(|result| {
                        matches!(
                            result["state"].as_str(),
                            Some("accepted" | "failed" | "rejected" | "deadline_exceeded")
                        )
                    }) {
                        worker.provider.speech = None;
                        worker.provider.ltx25_cache.get_mut().take();
                        worker.provider.wavespeed_cache.get_mut().take();
                    }
                    if done
                        .send((slot, outcome, std::mem::take(&mut worker.provider_pause)))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        drop(done);
        let mut busy = [false; LIMIT];
        let mut log = CycleLog::default();
        loop {
            for (slot, outcome, pauses) in finished.try_iter() {
                busy[slot] = false;
                for (provider, until) in pauses {
                    let current = dispatcher.provider_pause.entry(provider).or_default();
                    *current = (*current).max(until);
                }
                log.record(outcome);
            }
            if stop.load(Ordering::Relaxed) {
                // Dropping the assignments channels lets each worker finish
                // its current grant before the scoped threads join.
                return Ok(());
            }
            if handlers.iter().any(|h| h.is_finished()) {
                return Err(Error("worker_slot_stopped"));
            }
            let slots: Vec<_> = (1..=LIMIT).filter(|slot| !busy[slot - 1]).collect();
            if !slots.is_empty() {
                match dispatcher.claim_slots(Some(&slots)) {
                    Ok(work) if work.is_null() => {
                        if slots.len() == LIMIT {
                            log.record(Ok(json!({"state":"idle"})));
                        }
                    }
                    Ok(work) if dispatcher.session["maxConcurrentJobs"] != LIMIT => {
                        log.record(dispatcher.execute_work(work, None));
                    }
                    Ok(work) => {
                        let slot = work["workerSlot"]
                            .as_u64()
                            .and_then(|id| usize::try_from(id).ok())
                            .filter(|id| slots.contains(id))
                            .ok_or(Error("invalid_worker_slot"))?;
                        let assignment = Assignment {
                            work,
                            session: dispatcher.session.clone(),
                            policy: dispatcher.provider.media_policy.clone(),
                            catalog: dispatcher.provider.catalog.clone(),
                        };
                        senders[slot - 1]
                            .send(assignment)
                            .map_err(|_| Error("worker_slot_stopped"))?;
                        busy[slot - 1] = true;
                        continue;
                    }
                    Err(error) => log.record(Err(error)),
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models,
        network::Request,
        protocol::{self, Keys},
    };
    use std::sync::{Arc, Mutex, atomic::AtomicUsize};
    use std::time::Instant;

    #[derive(Clone)]
    struct Coordinator {
        signing: Keys,
        worker: Keys,
        next: Arc<AtomicUsize>,
        receipts: Arc<Mutex<Vec<Value>>>,
        stop: Arc<AtomicBool>,
        parallel: bool,
    }
    impl Runtime for Coordinator {
        fn quote(&self, _: &[u8]) -> Result<Value> {
            panic!("session must remain valid through all completions")
        }
        fn post(&self, path: &str, body: &Value) -> Result<Value> {
            assert_eq!(path, "/v1/action");
            let req = protocol::verified(body, &self.worker.signing_key())?;
            let payload = &req["payload"];
            let result = match req["action"].as_str().unwrap() {
                "claim" => {
                    assert_eq!(payload.get("freeSlots").is_some(), self.parallel);
                    let index = self.next.fetch_add(1, Ordering::SeqCst);
                    if index >= 6 {
                        json!({"work":null})
                    } else {
                        let spec = models::spec(&json!({"prompt":"A red boat","seed":index}))?;
                        let mut work = json!({"minerId":"fixture","attemptId":format!("a{index}"),
                            "jobId":format!("j{index}"),"spec":spec,"inputHash":protocol::digest(&spec)?,
                            "deadline":crate::now()+60_000,"state":"offered"});
                        if self.parallel {
                            work["workerSlot"] = payload["freeSlots"][0].clone();
                        }
                        json!({"work":protocol::seal(&work,&self.worker.encryption_key(),
                            &format!("session:{}",req["nonce"].as_str().unwrap()))?})
                    }
                }
                "start" => {
                    let index: usize = payload["attemptId"].as_str().unwrap()[1..].parse().unwrap();
                    let spec = models::spec(&json!({"prompt":"A red boat","seed":index}))?;
                    json!({"permitted":true,"attemptId":payload["attemptId"],"inputHash":protocol::digest(&spec)?,"grantNonce":format!("grant{index}")})
                }
                "submitted" => json!({"ok":true}),
                "complete" => {
                    let receipt =
                        protocol::verified(&payload["receipt"], &self.worker.signing_key())?;
                    let mut receipts = self.receipts.lock().unwrap();
                    receipts.push(receipt.clone());
                    if receipts.len() == 6 {
                        self.stop.store(true, Ordering::SeqCst);
                    }
                    json!({"jobId":receipt["jobId"],"outputHash":receipt["outputHash"]})
                }
                action => panic!("unexpected action {action}"),
            };
            protocol::signed(
                &json!({"sessionId":"session","requestNonce":req["nonce"],"result":result}),
                &self.signing.signing,
            )
        }
    }

    #[derive(Default)]
    struct ProviderHttp {
        active: AtomicUsize,
        peak: AtomicUsize,
        submissions: AtomicUsize,
        parallel: bool,
    }
    impl Http for ProviderHttp {
        fn bytes(&self, r: Request) -> Result<Vec<u8>> {
            let response = if r.method == "POST" && r.url.starts_with("https://queue.fal.run/") {
                let body: Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                self.submissions.fetch_add(1, Ordering::SeqCst);
                // Keep the first batch inside the real provider submit path
                // until all five are in flight. Bound the wait on regressions.
                let expiry = Instant::now() + Duration::from_secs(5);
                while self.parallel
                    && self.peak.load(Ordering::SeqCst) < LIMIT
                    && Instant::now() < expiry
                {
                    thread::sleep(Duration::from_millis(5));
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                json!({"request_id":format!("ref{}", body["seed"])})
            } else if r.url.ends_with("/status") {
                json!({"status":"COMPLETED"})
            } else if r.url.starts_with("https://queue.fal.run/") && r.url.contains("/requests/ref")
            {
                let reference = r.url.split('/').next_back().unwrap();
                json!({"video":{"url":format!("https://file.fal.media/{reference}")}})
            } else if r.url.starts_with("https://file.fal.media/") {
                return Ok(r.url.split('/').next_back().unwrap().as_bytes().to_vec());
            } else {
                json!({})
            };
            Ok(response.to_string().into_bytes())
        }
    }

    #[test]
    fn six_signed_jobs_use_five_slots_or_one_on_a_legacy_coordinator() {
        for parallel in [true, false] {
            let http = ProviderHttp {
                parallel,
                ..Default::default()
            };
            let stop = Arc::new(AtomicBool::new(false));
            let runtime = Coordinator {
                signing: Keys::default(),
                worker: Keys::default(),
                next: Default::default(),
                receipts: Default::default(),
                stop: stop.clone(),
                parallel,
            };
            let mut session = json!({"sessionId":"session","expires":crate::now()+3_600_000});
            if parallel {
                session["maxConcurrentJobs"] = json!(LIMIT);
            }
            let mut worker = Worker {
                miner_id: "fixture".into(),
                coordinator_key: runtime.signing.signing_key(),
                chain: Value::Null,
                keys: runtime.worker.clone(),
                session,
                runtime: runtime.clone(),
                provider: Router::new(&http, json!({"FAL_KEY":"test-fixture-key-0123456789"})),
                provider_pause: Default::default(),
            };
            let (tx, rx) = mpsc::channel::<()>();
            thread::scope(|scope| {
                let watchdog_stop = stop.clone();
                scope.spawn(move || {
                    if rx.recv_timeout(Duration::from_secs(25)).is_err() {
                        watchdog_stop.store(true, Ordering::Relaxed);
                    }
                });
                run(&mut worker, &http, &stop).unwrap();
                let _ = tx.send(());
            });
            assert_eq!(
                http.peak.load(Ordering::SeqCst),
                if parallel { LIMIT } else { 1 }
            );
            assert_eq!(http.submissions.load(Ordering::SeqCst), 6);
            let receipts = runtime.receipts.lock().unwrap();
            assert_eq!(receipts.len(), 6);
            for i in 0..6 {
                let r = receipts
                    .iter()
                    .find(|r| r["jobId"] == format!("j{i}"))
                    .unwrap();
                assert_eq!(r["grantNonce"], format!("grant{i}"));
                assert_eq!(r["providerRef"], format!("ref{i}"));
                assert_eq!(r["outputHash"], protocol::sha(format!("ref{i}").as_bytes()));
            }
        }
    }
}

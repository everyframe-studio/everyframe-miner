"use strict";
// everycli ui — local setup guide and status dashboard.
// Everything shown comes from the same commands the terminal runs. Data is always
// inserted as text (never as HTML), and no request leaves 127.0.0.1.
// Only non-secret model notification acknowledgments are kept in localStorage.

const S = {
  token: null, session: null, overview: null, wallets: null, help: null,
  health: null, healthAt: 0, offers: null, offersError: null, balances: null, earnings: null,
  history: null, historyBusy: false, historyRequest: 0, historyStatus: "all", historyModel: "", historyModelDraft: "", historyCursors: [null], historyPage: 0, openAttempt: null, earningsBusy: false,
  cliUpdate: null, workerUpdate: null, hotkeySel: null, registration: null,
  workerAuto: null, workerAutoBusy: false, pendingOperations: new Map(),
  offerDrafts: new Map(), modelQuery: "", modelProvider: "", modelKind: "", computeLimit: "10",
  selectedOffers: new Set(), batchDiscount: "0", batchBusy: false, batchReport: null,
  batchReportTimer: null, navigationEpoch: 0, newModels: new Set(), modelSeen: new Map(),
  openStep: null, renaming: false, keyStatus: {}, workloads: null, removal: null, offerFilter: "ready", timers: [], refreshing: false,
};
try { S.token = sessionStorage.getItem("everycli-token"); } catch { /* private mode */ }

// ---------- tiny DOM helper (text only) ----------
function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k === "text") el.textContent = v;
    else if (k === "value") el.value = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}
const $ = (sel) => document.querySelector(sel);
const fmtUsd = (n, d = 2) => (typeof n === "number" && isFinite(n)) ? "$" + n.toFixed(d) : "—";
const fmtTime = (ms) => ms ? new Date(ms).toLocaleString() : "—";
const ago = (ms) => {
  if (!ms) return "never";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return s + "s ago"; if (s < 3600) return Math.round(s / 60) + " min ago";
  return Math.round(s / 3600) + " h ago";
};
function bar(pct) {
  const fill = h("span");
  fill.style.width = Math.max(0, Math.min(100, Math.round(pct))) + "%";
  return h("div", { class: "progress" }, fill);
}
const daysLeft = (ms) => ms ? Math.floor((ms - Date.now()) / 86400000) : null;

// ---------- API ----------
async function api(path, body) {
  const opts = { method: body ? "POST" : "GET", headers: { "x-everycli-token": S.token || "" } };
  if (body) { opts.headers["content-type"] = "application/json"; opts.body = JSON.stringify(body); }
  let res;
  const controller = new AbortController(); opts.signal = controller.signal;
  const timeout = setTimeout(() => controller.abort(), path.startsWith("/api/keys") ? 45000 : 15000);
  try { res = await fetch(path, opts); } catch {
    return { ok: false, status: 0, data: { error: { code: "ui_offline", message: "The UI server is not reachable. Is everycli ui still running?" } } };
  } finally { clearTimeout(timeout); }
  let data = {};
  try { data = await res.json(); } catch { /* empty */ }
  if (res.status === 401) { forgetToken(); renderUnpaired(); }
  return { ok: res.ok, status: res.status, data };
}
function forgetToken() { S.token = null; try { sessionStorage.removeItem("everycli-token"); } catch { /* */ } }

const LABELS = {
  "status": "Status", "health": "Health check", "doctor": "Doctor", "providers": "Providers", "balances": "Balances",
  "offers": "Model offers", "earnings": "Earnings", "register-hotkey": "Check hotkey registration",
  "worker-update-check": "Check for worker update", "cli-update-check": "Check for CLI update", "init": "Create miner profile",
  "offer": "Update model offer", "deploy": "Deploy worker", "start": "Start worker", "activate": "Activate keys",
  "offer-batch": "Update model offers", "jobs": "Job history",
  "apply-api-keys": "Apply keys to worker", "resume": "Start accepting work", "stop": "Stop worker",
  "worker-update": "Update worker", "reconcile": "Re-sync operation", "remove-api-key": "Remove key",
  "balances-publish": "Publish balances",
  "label": "Save worker label", "logs": "Worker logs",
  "reconnect": "Reconnect miner profile",
};

// Runs a CLI command. Questions come back to the browser; approving one re-runs
// the command with that exact question allowed.
async function run(action, params = {}, opts = {}) {
  const changes = new Set(["register-hotkey", "init", "reconnect", "reconcile", "offer", "offer-batch", "deploy", "start", "activate", "apply-api-keys", "resume", "stop", "label", "delete", "worker-update", "worker-auto-enable", "worker-auto-disable", "remove-api-key", "balances-publish"]);
  if (changes.has(action) && action !== "reconcile" && !(action === "stop" && params.force) && S.pendingOperations.size) {
    const job = { state: "unknown", result: { error: { code: "ui_operation_unconfirmed", message: "Check the existing operation before starting another change." } } };
    if (!opts.quiet) Activity.add(opts.label || LABELS[action] || action).finish(job);
    return job;
  }
  const approvals = [...(opts.approvals || [])];
  const act = opts.quiet ? null : Activity.add(opts.label || LABELS[action] || action);
  for (;;) {
    const requestId = [...crypto.getRandomValues(new Uint8Array(16))].map(x => x.toString(16).padStart(2, "0")).join("");
    const operation = { id: requestId, action, title: opts.label || LABELS[action] || action, started: Date.now() };
    if (changes.has(action)) { S.pendingOperations.set(requestId, operation); savePendingOperations(); }
    const r = await api("/api/run", { action, params, approvals, requestId });
    if (!r.ok) {
      const uncertain = changes.has(action) && (r.status === 0 || r.status >= 500 || r.status === 408);
      if (!uncertain) { S.pendingOperations.delete(requestId); savePendingOperations(); }
      const job = uncertain ? unknownOperation(requestId) : { state: "failed", result: { error: r.data.error || { message: "Request failed." } } };
      act?.finish(job); return job;
    }
    const jobId = typeof r.data.job === "string" ? r.data.job : requestId;
    if (changes.has(action) && jobId !== requestId) {
      S.pendingOperations.delete(requestId); S.pendingOperations.set(jobId, { ...operation, id: jobId }); savePendingOperations();
    }
    const job = await poll(jobId, act);
    if (job.state !== "unknown") { S.pendingOperations.delete(jobId); savePendingOperations(); }
    if (job.state === "confirm") {
      // Batch review gathers exact CLI questions without approving anything.
      // If a quote changes after review, stop instead of silently authorizing it.
      if (opts.prepareOnly || opts.exactApproval) return job;
      const yes = await confirmDialog(job.result.question, opts.label || LABELS[action] || action);
      if (!yes) { act?.cancel(); return { state: "cancelled" }; }
      approvals.push(job.result.hash);
      continue;
    }
    act?.finish(job);
    return job;
  }
}
async function poll(id, act) {
  let failures = 0;
  for (;;) {
    const r = await api("/api/jobs/" + id);
    if (!r.ok) {
      if ((r.status === 0 || r.status >= 500 || r.status === 429) && failures++ < 2) {
        await new Promise(ok => setTimeout(ok, 500 * 2 ** failures)); continue;
      }
      return unknownOperation(id);
    }
    failures = 0;
    act?.log(r.data.log || []);
    if (r.data.state !== "running") return r.data;
    await new Promise((ok) => setTimeout(ok, 700));
  }
}
function unknownOperation(id) {
  if (S.pendingOperations.has(id)) {
    S.pendingOperations.get(id).unconfirmed = true;
    savePendingOperations(); patchPendingBanner();
  }
  return { state: "unknown", operationId: id, result: { error: { code: "ui_operation_unconfirmed",
    message: "Connection lost. The operation's outcome is not confirmed.",
    next: "The worker operation may still be running. Check its result or re-sync the existing worker; do not repeat the change." } } };
}
function savePendingOperations() {
  try { sessionStorage.setItem("everycli-pending-operations", JSON.stringify({ token: S.token, operations: [...S.pendingOperations.values()] })); } catch { /* memory remains authoritative */ }
}
function pendingBanner() {
  const ops = [...S.pendingOperations.values()].filter(op => op.unconfirmed);
  if (!ops.length) return null;
  const banner = notice("warn", "An operation needs checking", ops.map(op => h("div", { class: "row" },
    h("span", null, op.title || op.action), button("Check result", () => checkOperation(op.id, Activity.add(op.title || op.action)), "small"))));
  banner.id = "pending-operations"; return banner;
}
function patchPendingBanner() {
  $("#pending-operations")?.remove();
  const banner = pendingBanner(); if (banner && S.token) $("#main").prepend(banner);
}
async function checkOperation(id, activity) {
  const job = await poll(id, activity);
  if (job.state !== "unknown") { S.pendingOperations.delete(id); savePendingOperations(); patchPendingBanner(); }
  activity?.finish(job.state === "confirm" ? { state: "attention", result: { value: {
    message: "Additional confirmation is required. Review the worker state and run the action again to continue." } } } : job);
  if (job.state !== "unknown") await refreshAll();
}
function restorePendingOperations() {
  try {
    const saved = JSON.parse(sessionStorage.getItem("everycli-pending-operations"));
    if (saved?.token !== S.token || !Array.isArray(saved.operations)) return;
    for (const op of saved.operations.slice(0, 20)) {
      if (!/^[a-f0-9]{32}$/.test(op.id) || typeof op.action !== "string") continue;
      S.pendingOperations.set(op.id, { ...op, unconfirmed: true });
      Activity.add(op.title || op.action).finish(unknownOperation(op.id));
    }
  } catch { /* malformed or expired tab state */ }
}
const value = (job) => job && job.result && job.result.value;
const errorOf = (job) => job && job.result && job.result.error;

// ---------- activity panel ----------
const Activity = {
  add(title) {
    const log = h("pre", { class: "log", hidden: true });
    const msg = h("div", { class: "msg" }, "Working…");
    const icon = h("span", { class: "spin" });
    const close = h("button", { class: "icon-x", "aria-label": "Dismiss", onclick: () => box.remove() }, "×");
    const toggle = h("button", { class: "btn small", onclick: () => { log.hidden = !log.hidden; } }, "Details");
    const box = h("div", { class: "act" }, h("div", { class: "head" }, icon, h("span", { class: "t" }, title), toggle, close), msg, log);
    $("#activity").prepend(box);
    while ($("#activity").children.length > 5) $("#activity").lastChild.remove();
    const setIcon = (cls, text) => icon.replaceWith(Object.assign(icon, { className: "badge " + cls, textContent: text }));
    const activity = {
      log(lines) { log.textContent = lines.join("\n"); },
      cancel() { setIcon("warn", "STOPPED"); msg.textContent = "Cancelled. Nothing was changed by this step."; setTimeout(() => box.remove(), 6000); },
      finish(job) {
        const err = errorOf(job), v = value(job);
        close.hidden = job.state === "unknown" && S.pendingOperations.has(job.operationId);
        if (job.state === "unknown") {
          if (S.pendingOperations.has(job.operationId)) {
            S.pendingOperations.get(job.operationId).unconfirmed = true; savePendingOperations(); patchPendingBanner();
          }
          setIcon("warn", "UNCONFIRMED");
          msg.replaceChildren(errorView(err));
          if (job.operationId) msg.append(h("div", { class: "row" },
            button("Check result", () => checkOperation(job.operationId, activity), "small"),
            h("a", { class: "btn small", href: "#/worker" }, "Open Worker"),
            button("Clear after checking", async () => {
              if (await confirmDialog("Have you checked the existing worker and reconciled its state? This only clears the local notice; it does not stop or repeat the operation.", "Clear unconfirmed operation")) {
                S.pendingOperations.delete(job.operationId); savePendingOperations(); patchPendingBanner(); box.remove();
              }
            }, "small")));
        } else if (job.state === "done") {
          setIcon("ok", "DONE");
          msg.textContent = (v && typeof v.next === "string") ? v.next : "Finished.";
          setTimeout(() => { if (log.hidden) box.remove(); }, 9000);
        } else if (job.state === "attention") {
          setIcon("warn", "CHECK");
          msg.textContent = (v && (v.message || v.next)) || "Finished, but something needs your attention.";
        } else {
          setIcon("bad", "FAILED");
          msg.replaceChildren(errorView(err));
        }
        if (job.result && job.result.text) {
          log.textContent = (log.textContent ? log.textContent + "\n\n" : "") + job.result.text;
        }
      },
    };
    return activity;
  },
};
function errorView(err) {
  if (!err) return h("span", null, "Something went wrong.");
  return h("div", null,
    h("div", { class: "err-msg" }, err.message || err.code || "Something went wrong."),
    err.next ? h("div", { class: "small muted" }, err.next) : null,
    (err.error || err.code) === "coordinator_auth_rejected" ? h("a", { class: "btn", href: "#/worker" }, "Reconnect from Worker page") : null,
    err.registrationCommand ? copyBox(err.registrationCommand) : null,
    err.error || err.code ? h("div", { class: "small muted mono" }, "Code: " + (err.error || err.code)) : null);
}

// ---------- confirm dialog ----------
function confirmDialog(question, title) {
  return new Promise((resolve) => {
    const wrap = $("#modal");
    const previousFocus = document.activeElement;
    const input = h("input", { type: "text", placeholder: "yes", autocomplete: "off", "aria-label": "Type yes to continue" });
    const ok = h("button", { class: "btn primary", disabled: true }, "Continue");
    const done = (v) => { wrap.hidden = true; wrap.replaceChildren(); $("#app").inert = false; $("#activity").inert = false; document.removeEventListener("keydown", esc); previousFocus?.focus(); resolve(v); };
    const esc = (e) => {
      if (e.key === "Escape") done(false);
      if (e.key === "Tab") {
        const controls = [...wrap.querySelectorAll("button:not(:disabled), input")];
        const next = controls[(controls.indexOf(document.activeElement) + (e.shiftKey ? -1 : 1) + controls.length) % controls.length];
        e.preventDefault(); next?.focus();
      }
    };
    input.addEventListener("input", () => { ok.disabled = input.value.trim().toLowerCase() !== "yes"; });
    input.addEventListener("keydown", (e) => { if (e.key === "Enter" && !ok.disabled) done(true); });
    ok.addEventListener("click", () => done(true));
    wrap.replaceChildren(h("div", { class: "modal", role: "dialog", "aria-modal": "true", "aria-label": "Confirm" },
      h("h2", null, "Please confirm: " + title),
      h("div", { class: "question" }, question),
      h("div", { class: "field" }, h("label", null, "Type yes to continue (same as the terminal)"), input),
      h("div", { class: "row end", style: null }, h("button", { class: "btn", onclick: () => done(false) }, "Cancel"), ok)));
    wrap.hidden = false;
    $("#app").inert = true; $("#activity").inert = true;
    document.addEventListener("keydown", esc);
    input.focus();
  });
}

// ---------- small views ----------
function copyBox(text) {
  const btn = h("button", { class: "btn small" }, "Copy");
  btn.addEventListener("click", async () => {
    try { await navigator.clipboard.writeText(text); btn.textContent = "Copied"; } catch { btn.textContent = "Select and copy"; }
  });
  return h("div", { class: "copy" }, h("code", null, text), btn);
}
function notice(kind, title, body) { return h("div", { class: "notice " + (kind || "") }, title ? h("b", null, title) : null, body); }
function stat(label, val, sub, tone) { return h("div", { class: "stat " + (tone || "") }, h("div", { class: "label" }, label), h("div", { class: "value" }, val), sub ? h("div", { class: "sub" }, sub) : null); }
function card(title, actions, ...body) {
  return h("section", { class: "card" }, h("div", { class: "card-head" }, h("h2", null, title), actions ? h("div", { class: "row" }, actions) : null), ...body);
}
function checkState(c) { return c.state || (c.ok ? "PASS" : "FAIL"); }
function checksList(checks) {
  return h("ul", { class: "checks" }, (checks || []).map((c) => h("li", null,
    h("span", { class: "badge " + checkState(c) }, checkState(c)),
    h("div", null, h("div", { class: "name" }, c.name), h("div", { class: "detail" }, c.message || c.detail || "")))));
}
function healthList(checks) {
  const problems = checks.filter((c) => checkState(c) !== "PASS"), passed = checks.filter((c) => checkState(c) === "PASS");
  return h("div", null,
    problems.length ? checksList(problems) : notice("ok", "All checks pass", null),
    passed.length ? h("details", null, h("summary", null, passed.length + " checks passed"), checksList(passed)) : null);
}
function button(label, onclick, cls) {
  const b = h("button", { class: "btn " + (cls || "") }, label);
  b.addEventListener("click", async () => {
    b.disabled = true; b.dataset.busy = "true";
    try { await onclick(b); } finally { delete b.dataset.busy; if (b.isConnected) b.disabled = false; }
  });
  return b;
}
function field(label, input, hint) { return h("div", { class: "field" }, h("label", null, label), input, hint ? h("div", { class: "small muted" }, hint) : null); }

const PHASES = {
  not_deployed: ["Not deployed", ""], provision_intent: ["Creating (unconfirmed)", "warn"], provisioned: ["Provisioned", "warn"],
  commit_intent: ["Starting (unconfirmed)", "warn"], awaiting_attestation: ["Waiting for attestation", "warn"],
  activate_intent: ["Activating (unconfirmed)", "warn"], awaiting_readmission: ["Keys active · waiting for re-attestation", "warn"],
  running: ["Running", "ok"], drained: ["Paused", "warn"], draining: ["Pausing", "warn"], stop_intent: ["Stopping (unconfirmed)", "warn"],
  stopping: ["Shutting down", "warn"], stopped: ["Stopped", "bad"], start_intent: ["Starting (unconfirmed)", "warn"], starting: ["Starting", "warn"],
  delete_intent: ["Deleting (unconfirmed)", "warn"], deleting: ["Deleting", "warn"], deleted: ["Deleted", ""],
  update_provision_intent: ["Updating (unconfirmed)", "warn"], update_commit_intent: ["Updating (unconfirmed)", "warn"], updating: ["Updating", "warn"],
};
const phaseLabel = (p) => (PHASES[p] || [p || "Unknown", "warn"]);
const findCheck = (name) => S.health && (value(S.health)?.checks || []).find((c) => c.name === name);
function workerState() {
  const d = S.overview?.deployment || {}, p = d.phase || "not_deployed";
  if (["deleted", "not_deployed"].includes(p) || d.needsReconcile || /_intent$/.test(p)) return phaseLabel(p);
  const cloud = value(S.health)?.cloudStatus;
  if (["stopped", "exited"].includes(cloud)) return ["Stopped", "bad"];
  if (["stopping", "starting"].includes(cloud)) return [cloud === "stopping" ? "Shutting down" : "Starting", "warn"];
  if (p === "running" && (!S.health || errorOf(S.health) || !cloud || cloud === "unknown")) return ["Checking state", "warn"];
  return phaseLabel(p);
}
function workerControls() {
  const d = S.overview?.deployment || {}, p = d.phase || "not_deployed", h = value(S.health);
  const exists = !!d.appId && !["deleted", "not_deployed"].includes(p);
  const pending = !!d.needsReconcile || S.pendingOperations.size > 0 || /_intent$/.test(p) || ["starting", "stopping", "deleting", "updating"].includes(p);
  const stopped = ["stopped", "exited"].includes(h?.cloudStatus) || p === "stopped";
  const live = exists && !pending && !stopped;
  const ready = h && !errorOf(S.health) && findCheck("Workload attestation")?.ok === true;
  return {
    exists, pending, stopped,
    apply: live && ready && ["running", "drained", "awaiting_readmission"].includes(p),
    shutdown: live && ["running", "drained", "draining", "awaiting_attestation", "awaiting_readmission"].includes(p),
    pause: live && p === "running",
    start: exists && !pending && stopped,
    resume: live && ready && ["running", "drained", "awaiting_readmission"].includes(p) && findCheck("Accepting new work")?.ok === false,
  };
}

const PROVIDERS = {
  phala: ["Phala Cloud", "Hosting. Your worker runs in a sealed (TEE) machine billed to your Phala account."],
  fal: ["fal", "Most video, image and audio models on the network (for example H3 Max, Seedance 2.5, Flux 3)."],
  vivix: ["Vivix", "Vivix W1 world video. Needs Vivix W credits (a W credit pack); A credits do not work."],
  pika: ["Pika", "Pika H3 Flex video with native audio, keyframes and visual references. Use a Pika developer API key. Your API balance is checked without generating media."],
  kling: ["Kling (direct)", "Kling developer API key, not a Fal key or legacy access/secret pair. API resource packs are separate from the web subscription. Model availability requires a signed worker release and pricing."],
  openrouter: ["OpenRouter", "Seedance and image models served through OpenRouter."],
  minimax: ["MiniMax (direct)", "MiniMax models served by MiniMax's own API."],
  bfl: ["Black Forest Labs", "FLUX image models from BFL's own API."],
  replicate: ["Replicate", "Models hosted on Replicate."],
  google: ["Google", "Google models through Google's API."],
  runway: ["Runway", "Runway video models."],
  luma: ["Luma", "Luma video models."],
  elevenlabs: ["ElevenLabs", "Speech and sound models from ElevenLabs."],
  "fal-billing": ["fal billing (optional)", "An admin key used only on this computer to show your fal balance. Never sent to the worker."],
  "openrouter-billing": ["OpenRouter billing (optional)", "Used only on this computer to show your OpenRouter balance. Never sent to the worker."],
};
const providerName = (p) => (PROVIDERS[p] || [p])[0];

// ---------- data loading ----------
async function loadOverview() {
  const r = await api("/api/overview");
  if (r.ok) S.overview = r.data;
  return S.overview;
}
async function loadHealth(quiet = true) {
  if (!S.overview?.initialized) return;
  S.health = await run("health", {}, { quiet });
  S.healthAt = Date.now();
}
async function loadBalances() {
  S.balances = await run("balances", {}, { quiet: true });
  if (currentRoute() === "keys") renderBalancePanel();
}
function renderBalancePanel() { $("#provider-balances")?.replaceWith(balancesCard()); }
async function loadEarnings() {
  if (!S.overview?.initialized || S.earningsBusy) return;
  S.earningsBusy = true;
  try { S.earnings = await run("earnings", {}, { quiet: true }); } finally { S.earningsBusy = false; }
}
async function loadHistory(reset = false) {
  if (!S.overview?.initialized) return;
  if (reset) { S.historyCursors = [null]; S.historyPage = 0; }
  const request = ++S.historyRequest;
  const params = { limit: "20", status: S.historyStatus };
  if (S.historyModel) params.model = S.historyModel;
  if (S.historyCursors[S.historyPage]) params.cursor = S.historyCursors[S.historyPage];
  S.historyBusy = true; S.openAttempt = null;
  if (currentRoute() === "jobs") render();
  const result = await run("jobs", params, { quiet: true });
  if (request !== S.historyRequest) return;
  S.history = result; S.historyBusy = false;
  if (currentRoute() === "jobs") render();
}
async function loadOffers(quiet = true) {
  const job = await run("offers", {}, { quiet });
  S.offers = value(job); S.offersError = errorOf(job);
  if (Array.isArray(S.offers?.offers)) trackModels(S.offers.offers);
}
const integratedModel = r => r.contractAvailable !== false && !(r.blockers || []).includes("integration_required");
function modelSeenKey() { return "everycli-models-seen:v1:" + (S.session?.network || "mainnet") + ":" + (S.overview?.profile?.minerId || "public"); }
function saveSeenModels(key, ids) {
  S.modelSeen.set(key, new Set(ids));
  try { localStorage.setItem(key, JSON.stringify([...ids])); } catch { /* memory fallback */ }
}
function trackModels(rows) {
  const key = modelSeenKey(), ids = rows.filter(integratedModel).map(r => r.model).filter(id => typeof id === "string");
  let seen = S.modelSeen.get(key);
  if (!seen) {
    try { const stored = JSON.parse(localStorage.getItem(key)); if (Array.isArray(stored) && stored.length <= 1024 && stored.every(id => typeof id === "string")) seen = new Set(stored); } catch { /* private mode or invalid cache */ }
  }
  // Establish a baseline, not a misleading "all models are new" notification.
  if (!seen) { saveSeenModels(key, ids); S.newModels.clear(); return; }
  S.modelSeen.set(key, seen);
  S.newModels = new Set(ids.filter(id => !seen.has(id)));
}
function acknowledgeModels() {
  if (!Array.isArray(S.offers?.offers)) return;
  saveSeenModels(modelSeenKey(), S.offers.offers.filter(integratedModel).map(r => r.model));
  S.newModels.clear();
}

// ---------- shell ----------
const ROUTES = [["overview", "Overview"], ["setup", "Setup guide"], ["models", "Models"], ["jobs", "Job history"], ["earnings", "Earnings"], ["keys", "API keys"], ["worker", "Worker"], ["help", "Help"]];
function route() { const m = location.hash.match(/^#\/(\w+)/); return m && ROUTES.some(([r]) => r === m[1]) ? m[1] : null; }
function currentRoute() { return route() || (S.overview?.initialized ? "overview" : "setup"); }
function go(r) { location.hash = "#/" + r; }

function renderMeta() {
  const s = S.session, o = S.overview;
  if (!s) return;
  const [label, tone] = workerState();
  $("#meta").replaceChildren(...[
    h("span", { class: "pill brand-pill" }, (s.network === "testnet" ? "Testnet · SN" : "Mainnet · SN") + s.netuid),
    o?.initialized ? h("span", { class: "pill " + tone }, h("span", { class: "dot" }), "Worker: " + label) : null,
    h("span", { class: "pill hide-sm" }, "everycli " + s.version),
    S.cliUpdate && value(S.cliUpdate)?.updateAvailable ? h("span", { class: "pill warn" }, h("span", { class: "dot" }), "CLI update available") : null].filter(Boolean));
}
function renderNav() {
  const current = currentRoute();
  const steps = S.overview?.steps || [];
  const done = steps.filter((x) => stepDone(x)).length;
  $("#nav").replaceChildren(...ROUTES.map(([r, label]) => h("a", { href: "#/" + r, class: r === current ? "active" : "" },
    label, r === "models" && S.newModels.size ? h("span", { class: "count", "aria-label": S.newModels.size + " new models" }, S.newModels.size) : null,
    r === "setup" && steps.length ? h("span", { class: "count" }, done + "/" + steps.length) : null)));
}
function render() {
  if (!S.token) return renderUnpaired();
  renderMeta(); renderNav();
  const r = currentRoute();
  const page = { overview: pageOverview, setup: pageSetup, models: pageModels, jobs: pageJobs, earnings: pageEarnings, keys: pageKeys, worker: pageWorker, help: pageHelp }[r];
  $("#main").replaceChildren(page());
  patchPendingBanner();
}
function renderUnpaired() {
  $("#meta").replaceChildren(); $("#nav").replaceChildren();
  $("#main").replaceChildren(h("div", { class: "hero" },
    h("h1", null, "Connect this page"),
    h("p", { class: "lede" }, "For your safety, this page only works when opened from the link that everycli ui prints in your terminal. Each link works once; opening one prints a fresh link for the next tab."),
    copyBox("everycli ui"),
    h("p", { class: "small muted" }, "On a remote server, forward the port first: ssh -N -L 47117:127.0.0.1:47117 <user>@<server>")));
}

// ---------- worker name and label ----------
// The signed worker name is long (everyframe-hk-<40 hex>); show "every…ec7d96" and
// keep the full value in the tooltip and on the Worker page.
function shortName(name) {
  if (!name) return "";
  return name.length > 16 ? name.slice(0, 5) + "…" + name.slice(-6) : name;
}
function displayName() {
  const o = S.overview;
  return o?.label || shortName(o?.profile?.workerName) || "";
}
function renameButton() {
  return h("button", { class: "linkish", title: "Give this worker a name shown in everycli and here. The signed worker identity stays the same.",
    onclick: () => { S.renaming = true; render(); } }, S.overview?.label ? "Rename" : "Name this worker");
}
function renameForm() {
  const input = h("input", { type: "text", value: S.overview?.label || "", maxlength: "64", placeholder: "e.g. studio-worker-1", "aria-label": "Worker label" });
  const status = h("div", { class: "small" });
  const save = async () => {
    const name = input.value.trim();
    if (!name) { status.textContent = "Type a name first."; return; }
    const job = await run("label", { name }, { quiet: true });
    if (job.state !== "done") { status.replaceChildren(errorView(errorOf(job))); return; }
    S.renaming = false; await loadOverview(); render();
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") save(); if (e.key === "Escape") { S.renaming = false; render(); } });
  setTimeout(() => input.focus(), 0);
  return h("div", { class: "rename" }, input,
    h("div", { class: "row" }, button("Save", save, "small primary"), h("button", { class: "btn small", onclick: () => { S.renaming = false; render(); } }, "Cancel")),
    status);
}
function workerStat(phase, tone) {
  const full = S.overview?.profile?.workerName || "";
  return h("div", { class: "stat " + (tone || "") },
    h("div", { class: "label" }, "Worker"),
    h("div", { class: "value" }, phase),
    S.renaming ? renameForm() : [h("div", { class: "sub ellipsis", title: full }, displayName() || "—"),
      h("div", { class: "sub" }, renameButton())]);
}
function nameStat() {
  const p = S.overview?.profile || {};
  return h("div", { class: "stat" },
    h("div", { class: "label" }, "Worker name"),
    S.renaming ? renameForm() : h("div", { class: "value ellipsis" + (S.overview?.label ? "" : " muted"), title: S.overview?.label || "" }, S.overview?.label || "Not named yet"),
    S.renaming ? null : h("div", { class: "sub ellipsis mono", title: p.workerName || "" }, shortName(p.workerName) || "—"),
    S.renaming ? null : h("div", { class: "sub" }, renameButton()));
}

// ---------- Overview ----------
function minerIdentity() {
  const o = S.overview, key = o?.profile?.hotkey || o?.registration?.hotkey;
  if (!key) return null;
  const r = o.registration;
  const matched = r?.registered === true && r.hotkey === key && r.network === o.network;
  const uid = matched && Number.isInteger(r.uid) && r.uid >= 0 && r.uid <= 65535 ? r.uid : null;
  const copy = button("Copy hotkey", async (b) => {
    try { await navigator.clipboard.writeText(key); b.textContent = "Copied"; }
    catch { b.after(copyBox(key)); b.textContent = "Select and copy"; }
  }, "small");
  return h("div", { class: "row miner-identity", "aria-label": "Miner identity" },
    h("span", { class: "pill", title: uid == null ? "Refresh registration to look up the current UID" : "Registration last checked " + fmtTime(r.checkedAt) }, uid == null ? "UID not checked" : "UID " + uid),
    h("code", { title: key, "aria-label": "Public hotkey " + key }, key.slice(0, 6) + "…" + key.slice(-6)), copy,
    button("Refresh UID", async () => { await run("register-hotkey", { hotkeySs58: key }); await loadOverview(); render(); }, "small"));
}
function reconnectButton() {
  return button("Reconnect profile", async () => { await run("reconnect", S.hotkeySel || {}); await refreshAll(); });
}
function workStatusStat() {
  if (!S.health) return stat("Status", "Checking…", "Fetching worker status");
  const health = value(S.health), checks = health?.checks || [];
  if (errorOf(S.health) || !health || !checks.some(c => c.name === "Accepting new work")) {
    return stat("Status", "Unknown", "Could not verify availability", "warn");
  }
  const accepting = checks.find(c => c.name === "Accepting new work");
  if (checks.some(c => c.name !== "Accepting new work" && checkState(c) === "FAIL")) {
    return stat("Status", "Needs attention", "See health checks below", "warn");
  }
  if (!accepting.ok) return stat("Status", "Paused", "Not accepting new jobs", "warn");
  if (health.ok !== true) return stat("Status", "Needs attention", "See health checks below", "warn");
  return health.activity?.activeJobs > 0
    ? stat("Status", "Working", "Processing jobs", "ok")
    : stat("Status", "Ready", "Accepting new jobs", "ok");
}
function pageOverview() {
  const o = S.overview;
  const root = h("div", null, h("h1", null, "Overview"),
    h("p", { class: "lede" }, "Live status of your miner. Refreshes every 30 seconds while this tab is open."));
  if (!o) return root;
  root.append(...[minerIdentity()].filter(Boolean));
  if (!o.initialized) {
    const steps = o.steps || [], done = steps.filter(stepDone).length;
    root.append(h("div", { class: "hero" },
      h("h2", null, "Let's get your miner running"),
      h("p", { class: "lede" }, "The setup guide walks you through each step, explains what it costs, and asks before anything is paid."),
      bar((done / Math.max(1, steps.length)) * 100),
      h("p", { class: "small muted" }, done + " of " + steps.length + " steps done"),
      h("a", { class: "btn primary", href: "#/setup" }, "Continue setup")));
    return root;
  }
  const health = value(S.health), checks = health?.checks || [];
  if (o.recoveryPending || checks.some(c => c.detail === "coordinator_auth_rejected" || c.code === "coordinator_auth_rejected")) {
    root.append(notice("warn", "Reconnect your saved profile", h("div", null,
      h("p", null, "Refresh miner authorization using the same local hotkey. Your API keys and deployment history are kept; no VM is deployed automatically."), reconnectButton())));
  }
  const [phase, tone] = workerState();
  const activity = health?.activity || {};
  const activeOffers = checks.filter((c) => (c.name || "").startsWith("Model: ")).length;
  root.append(h("div", { class: "grid overview-stats", "aria-label": "Miner overview summary" },
    workerStat(phase, tone),
    workStatusStat(),
    stat("Active offers", String(activeOffers), activeOffers ? "" : "Choose models to serve", activeOffers ? "ok" : "warn"),
    stat("Jobs", activity.activeJobs == null ? "Unknown" : activity.activeJobs + " active", activity.acceptedJobs == null ? "Accepted count not reported" : activity.acceptedJobs + " accepted so far")));
  if (S.workloads?.length) root.append(notice("warn", "A worker for this miner runs in your Phala account, but this computer doesn't manage it",
    h("div", null, "It was probably deployed from another computer. Keep managing it there, or delete or replace it from the ", h("a", { href: "#/worker" }, "Worker page"), ".")));
  if (o.deployment.needsReconcile) root.append(notice("warn", "An operation was interrupted", "Your last change didn't finish cleanly. Re-sync it from the Worker page before anything else; never deploy a second worker."));
  root.append(card("Health checks", [button("Refresh", async () => { await loadHealth(false); render(); })],
    S.health ? (errorOf(S.health) ? errorView(errorOf(S.health)) : healthList(checks)) : h("p", { class: "muted" }, "Checking…"),
    h("p", { class: "small muted" }, "All green means the current readiness checks passed. It does not guarantee traffic, provider funds or earnings.")));
  const jobs = activity.jobsByModel;
  root.append(card("Jobs by model", [h("a", { class: "btn small", href: "#/jobs" }, "Individual job history"), h("span", { class: "small muted" }, "All time")],
    errorOf(S.health) ? errorView(errorOf(S.health)) : !Array.isArray(jobs)
      ? h("p", { class: "empty" }, "Per-model job counts have not been reported yet.")
      : jobs.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm" },
        h("thead", null, h("tr", null, ["Model", "Completed", "Active", "Failed attempts", "Cancelled / expired"].map(label => h("th", null, label)))),
        h("tbody", null, jobs.map(j => h("tr", null,
          h("td", { "data-label": "Model" }, h("b", null, S.offers?.offers?.find(r => r.model === j.model)?.name || j.model)),
          ...[["Completed", j.completed], ["Active", j.active], ["Failed attempts", j.failed], ["Cancelled / expired", j.cancelled]].map(([label, n]) => h("td", { "data-label": label }, Number.isSafeInteger(n) ? n.toLocaleString() : "—")))))))
        : h("p", { class: "empty" }, "No jobs yet. Your completed jobs will appear here."),
    h("p", { class: "small muted" }, "Completed means accepted, verified output—not a payment or an alpha reward. Failed and cancelled counts describe attempts.")));
  root.append(balancesCard(), earningsCard());
  return root;
}
function balanceAmount(r) {
  const native = r.nativeBalance;
  if (r.provider === "vivix" && native?.unit === "W_CREDITS" && Number.isSafeInteger(native.remainingMicrounits)) {
    return (native.remainingMicrounits / 1e6).toLocaleString(undefined, { maximumFractionDigits: 6 }) + " W credits";
  }
  return r.remainingUsd != null ? fmtUsd(r.remainingUsd) : "—";
}
function balancesCard() {
  const v = value(S.balances), rows = v?.balances || [];
  const panel = card("Provider balances", [button("Refresh", async () => { await loadBalances(); if (currentRoute() !== "keys") renderBalancePanel(); })],
    !S.balances ? h("p", { class: "muted" }, "Loading…") : errorOf(S.balances) ? errorView(errorOf(S.balances)) : rows.length
      ? h("div", { class: "table-wrap" }, h("table", null, h("thead", null, h("tr", null, h("th", null, "Provider"), h("th", null, "Remaining"), h("th", null, "Status"), h("th", null, "Checked"))),
        h("tbody", null, rows.map((r) => h("tr", null, h("td", null, providerName(r.provider)),
          h("td", null, balanceAmount(r)),
          h("td", null, h("span", { class: "badge " + (r.status === "ok" && !r.stale ? "ok" : "warn") }, r.status || "—"), r.stale ? h("span", { class: "small muted" }, " stale") : null,
            r.diagnostic ? errorView(r.diagnostic) : null),
          h("td", null, r.checkedAt ? ago(r.checkedAt) : "—"))))))
      : h("p", { class: "empty" }, "Save a provider key to check its balance. Some providers need a separate billing key."),
    v?.note ? h("p", { class: "small muted" }, v.note) : null);
  panel.id = "provider-balances";
  return panel;
}
function earningsCard() {
  const v = value(S.earnings);
  return card("Work value · not payments", [h("a", { class: "btn small", href: "#/earnings" }, "View earnings report")],
    !S.earnings ? h("p", { class: "muted" }, "Loading…") : errorOf(S.earnings) ? errorView(errorOf(S.earnings)) : [
      h("dl", { class: "kv" }, h("dt", null, "Pending review"), h("dd", null, reportUsd(v, v?.summary?.pendingReviewMicrousd)),
        h("dt", null, "Approved work"), h("dd", null, reportUsd(v, v?.summary?.approvedMicrousd)),
        h("dt", null, "Alpha received"), h("dd", null, "Not measured")),
      h("p", { class: "small muted" }, "These are accounting values, not wallet payments or net profit. Actual provider bills and hosting costs are not measured."),
      v?.asOf ? h("p", { class: "small muted" }, "Updated " + ago(v.asOf)) : null]);
}

// ---------- Private job history and accounting ----------
const ATTEMPT_LABELS = { offered: "Assigned", starting: "Starting", submitted: "Generating", unknown: "Submission uncertain", accepted: "Completed", failed: "Failed", rejected: "Not generated", timed_out: "Timed out", cancelled: "Cancelled", expired: "Expired" };
const REVIEW_LABELS = { review: "Pending review", approved: "Approved", disputed: "Excluded", not_applicable: "Not applicable" };
const ELIGIBILITY_LABELS = { pending_review: "Pending review", marked_eligible: "Marked eligible", not_eligible: "Not marked eligible", excluded: "Excluded", not_applicable: "Not applicable" };
const FAILURE_LABELS = { provider_auth_rejected: "Provider rejected the API key", provider_billing_required: "Provider billing requires attention", provider_capacity_limited: "Provider capacity unavailable", provider_key_required: "Provider key missing", provider_model_unavailable: "Provider model unavailable", provider_invalid_request: "Provider rejected the request", provider_content_rejected: "Provider declined the content", provider_timeout: "Provider timed out", provider_unavailable: "Provider unavailable", output_invalid: "Output failed validation", deadline_exceeded: "Job deadline exceeded", generation_failed: "Generation failed", provider_generation_failed: "Provider generation failed", request_timeout: "Submission timed out; charges may be uncertain", submission_unknown: "Submission outcome unknown" };
function reportUsd(v, amount) { return v?.currency === "USD" && v?.unit === "microusd" && Number.isSafeInteger(amount) && amount >= 0 ? new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 6 }).format(amount / 1e6) : "Not reported"; }
function reportDate(t) { return Number.isSafeInteger(t) && t > 0 ? new Date(t).toLocaleString(undefined, { timeZone: "UTC" }) + " UTC" : "Not recorded"; }
function historyUsd(amount) { return reportUsd({ currency: "USD", unit: "microusd" }, amount); }
function historyDetail(row) {
  const processing = Number.isSafeInteger(row.startedAt) && Number.isSafeInteger(row.completedAt) && row.completedAt >= row.startedAt ? ((row.completedAt - row.startedAt) / 1000).toFixed(1) + " seconds (start grant to accepted output)" : "Not recorded";
  const details = card("Attempt details", [button("Close details", () => { S.openAttempt = null; render(); }, "small")],
    h("dl", { class: "kv job-kv" }, ...[
      ["Job ID", row.jobId], ["Attempt ID", row.attemptId], ["Model", row.name || row.model], ["Contract ID", row.model],
      ["Provider", row.provider ? providerName(row.provider) : "Not reported"], ["Attempt", ATTEMPT_LABELS[row.state] || "Not reported"],
      ["Job created", reportDate(row.jobCreatedAt)], ["Start grant", reportDate(row.startedAt)], ["Output accepted", reportDate(row.completedAt)],
      ["Failure reported", reportDate(row.failedAt)], ["Processing time", processing],
      ["Requested media duration", row.durationSeconds != null ? row.durationSeconds + " seconds" : "Not reported"], ["Resolution", row.resolution || "Not reported"],
      ["Output verification", row.verification === "accepted" ? "Accepted" : "Not verified"], ["Review", REVIEW_LABELS[row.reviewState] || "Not reported"],
      ["Review decision", reportDate(row.reviewedAt)], ["Reward flag", ELIGIBILITY_LABELS[row.rewardEligibility] || "Not reported"],
      ["Recorded work value", historyUsd(row.recordedWorkValueMicrousd)], ["Provider quote estimate", historyUsd(row.providerCostEstimateMicrousd)],
      ["Failure reason", row.failureReason ? ({ miner_silent: "Worker heartbeat stopped", submission_timeout: "Worker did not confirm submission in time", attempt_timed_out: "Attempt timed out; exact cause not recorded", offer_lease_expired: "Assignment was not accepted in time" }[row.failureReason] || FAILURE_LABELS[row.failureReason] || "Failure reported (see code below)") : "Not reported"],
    ].flatMap(([label, text]) => [h("dt", null, label), h("dd", null, text || "Not reported")])),
    row.failureReason ? h("p", { class: "small muted mono" }, "Failure code: " + row.failureReason) : null,
    h("p", { class: "small muted" }, "Provider quote estimates are not bills. A failed or uncertain generation may still incur charges. Review flags do not prove epoch inclusion or alpha payment."));
  details.id = "job-detail"; details.tabIndex = -1; return details;
}
function pageJobs() {
  const root = h("div", null, h("h1", null, "Job history"), h("p", { class: "lede" }, "Every attempt by your miner, newest first. Inspect verification, review decisions and failures without exposing customer prompts or media."));
  if (!S.overview?.initialized) { root.append(notice("", "Connect your miner", "Complete setup to read this miner's private history.")); return root; }
  const status = h("select", { "aria-label": "Filter job status", disabled: S.historyBusy, onchange: e => { S.historyStatus = e.target.value; loadHistory(true); } },
    [["all", "All attempts"], ["active", "Active"], ["completed", "Completed"], ["failed", "Failed / rejected"], ["cancelled", "Cancelled / expired"]].map(([v, label]) => h("option", { value: v, selected: S.historyStatus === v }, label)));
  const model = h("input", { type: "text", value: S.historyModelDraft, placeholder: "Contract ID (optional)", "aria-label": "Filter job model", oninput: e => { S.historyModelDraft = e.target.value; } });
  const apply = () => { S.historyModel = S.historyModelDraft.trim(); loadHistory(true); };
  model.addEventListener("keydown", e => { if (e.key === "Enter" && !S.historyBusy) apply(); });
  root.append(h("div", { class: "row history-filters" }, status, model, h("button", { class: "btn", disabled: S.historyBusy, onclick: apply }, "Apply model filter"), h("button", { class: "btn", disabled: S.historyBusy, onclick: () => loadHistory(true) }, "Refresh")));
  const v = value(S.history), history = v?.history;
  if (S.historyBusy) root.append(h("p", { role: "status", class: "muted" }, "Loading job history…"));
  else if (errorOf(S.history)) root.append(notice("bad", "Could not load job history", errorView(errorOf(S.history))));
  else if (v?.historyAvailable === false) root.append(notice("warn", "History is not available yet", v.note || "This coordinator reports model counts only."));
  else if (Array.isArray(history?.rows)) {
    const rows = history.rows;
    root.append(card("Your attempts", [h("span", { class: "small muted" }, "Page " + (S.historyPage + 1))], rows.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm job-history" },
      h("thead", null, h("tr", null, ["Model", "Created", "Attempt", "Review", "Work value", "Inspect"].map(label => h("th", null, label)))),
      h("tbody", null, rows.map(row => h("tr", null,
        h("td", { "data-label": "Model" }, h("b", null, row.name || row.model), h("span", { class: "id" }, "Job " + String(row.jobId).slice(0, 12)), h("span", { class: "small muted" }, row.provider ? providerName(row.provider) : "Provider not reported")),
        h("td", { "data-label": "Created", title: reportDate(row.jobCreatedAt) }, Number.isSafeInteger(row.jobCreatedAt) ? ago(row.jobCreatedAt) : "Not recorded"),
        h("td", { "data-label": "Attempt" }, h("span", { class: "badge " + (row.state === "accepted" ? "ok" : ["failed", "rejected", "timed_out"].includes(row.state) ? "bad" : "warn") }, ATTEMPT_LABELS[row.state] || "Not reported")),
        h("td", { "data-label": "Review" }, REVIEW_LABELS[row.reviewState] || "Not reported"),
        h("td", { "data-label": "Work value" }, row.recordedWorkValueMicrousd == null ? "—" : historyUsd(row.recordedWorkValueMicrousd)),
        h("td", { "data-label": "Inspect" }, h("button", { class: "btn small", "aria-label": "View attempt " + row.attemptId, onclick: () => { S.openAttempt = row.attemptId; render(); $("#job-detail")?.focus({ preventScroll: true }); $("#job-detail")?.scrollIntoView({ block: "nearest", behavior: "smooth" }); } }, "Details"))
      ))))) : h("p", { class: "empty" }, S.historyStatus === "all" && !S.historyModel ? "No attempts yet. Your miner's first assignment will appear here." : "No attempts match these filters."),
      h("div", { class: "row end" }, h("button", { class: "btn small", disabled: S.historyPage === 0, onclick: () => { S.historyPage--; loadHistory(); } }, "Previous page"),
        h("button", { class: "btn small", disabled: !history.nextCursor, onclick: () => { S.historyCursors = [...S.historyCursors.slice(0, S.historyPage + 1), history.nextCursor]; S.historyPage++; loadHistory(); } }, "Next page")),
      h("p", { class: "small muted" }, "One row per attempt, not one payment. Work value is not cash or alpha received. Updated " + (v.asOf ? ago(v.asOf) : "time not reported") + ".")));
    const selected = rows.find(row => row.attemptId === S.openAttempt); if (selected) root.append(historyDetail(selected));
  }
  if (!S.history && !S.historyBusy) queueMicrotask(() => { if (!S.history && !S.historyBusy && currentRoute() === "jobs") loadHistory(); });
  return root;
}
function pageEarnings() {
  const root = h("div", null, h("h1", null, "Earnings"), h("p", { class: "lede" }, "All-time work accounting for this miner. Recorded work value, actual payments and estimated costs are different things."));
  if (!S.overview?.initialized) { root.append(notice("", "Connect your miner", "Complete setup to read this miner's private earnings.")); return root; }
  root.append(h("div", { class: "row end" }, button("Refresh earnings", async () => { await loadEarnings(); render(); })));
  if (!S.earnings) { root.append(h("p", { class: "muted" }, "Loading earnings…")); if (!S.earningsBusy) queueMicrotask(() => { if (!S.earnings && !S.earningsBusy && currentRoute() === "earnings") loadEarnings().then(render); }); return root; }
  if (errorOf(S.earnings)) { root.append(notice("bad", "Could not load earnings", errorView(errorOf(S.earnings)))); return root; }
  const v = value(S.earnings), summary = v?.summary, estimates = v?.estimates;
  root.append(notice("", "Work value is not a payment", "Approved work contributes to accounting. A reward-eligibility flag is not proof of inclusion in a finalized epoch or alpha received in your wallet."));
  if (summary) root.append(h("div", { class: "grid earnings-stats" },
    stat("Recorded work value", reportUsd(v, summary.totalWorkValueMicrousd), "All review states combined"),
    stat("Pending review", reportUsd(v, summary.pendingReviewMicrousd), "Verified outputs awaiting a decision"),
    stat("Approved work", reportUsd(v, summary.approvedMicrousd), "Reviewed accounting value, not cash"),
    stat("Marked reward-eligible", reportUsd(v, summary.markedRewardEligibleMicrousd), "Subset of approved work; do not add twice")));
  else root.append(notice("warn", "Detailed summary not reported", "This coordinator provides review totals only. Unknown amounts are not treated as zero."));
  const fees = Array.isArray(v?.fees) ? v.fees : [];
  root.append(card("Review breakdown", [h("a", { class: "btn small", href: "#/jobs" }, "Inspect individual jobs")], fees.length ? h("div", { class: "table-wrap" }, h("table", null,
    h("thead", null, h("tr", null, ["Review status", "Jobs", "Work value"].map(label => h("th", null, label)))),
    h("tbody", null, fees.map(row => h("tr", null, h("td", null, REVIEW_LABELS[row.state] || "Other accounting state"), h("td", null, Number.isSafeInteger(row.jobs) ? row.jobs.toLocaleString() : "Not reported"), h("td", null, reportUsd(v, row.amountMicrousd))))))) : h("p", { class: "empty" }, "No verified work value recorded yet."),
    h("p", { class: "small muted" }, "Excluded/disputed work is not approved. Pending review does not mean rejected. These amounts are accounting records, not payments.")));
  root.append(card("Actual income and costs", null, h("dl", { class: "kv" }, ...[
    ["Alpha received", "Not measured"], ["Provider bills", "Not measured"], ["Hosting costs", "Not measured"],
    ["Wallet payments", v?.payoutsImplemented === false ? "Not implemented" : "Not measured"], ["Net profit", "Unavailable — actual income and costs are not measured"],
  ].flatMap(([label, text]) => [h("dt", null, label), h("dd", null, text)])), h("p", { class: "small muted" }, "A provider balance is not per-miner spending. Shared-account balances cannot establish this miner's costs.")));
  if (estimates) root.append(card("Estimated provider exposure · not a bill", null,
    h("p", { class: "exposure-value" }, reportUsd(v, estimates.providerExposureMicrousd)),
    h("dl", { class: "kv" }, ...[["Attempts with quotes", estimates.quotedAttempts], ["Attempts missing a quote", estimates.unpricedAttempts], ["Uncertain submissions", estimates.uncertainAttempts]].flatMap(([label, n]) => [h("dt", null, label), h("dd", null, Number.isSafeInteger(n) ? n.toLocaleString() : "Not reported")])),
    h("p", { class: "small muted" }, "Conservative locked-job quotes for confirmed or uncertain submissions, including failed attempts. Actual charges may differ. Missing quotes and other account activity are excluded; this estimate is never presented as profit.")));
  if (v?.asOf) root.append(h("p", { class: "small muted" }, "Report updated " + reportDate(v.asOf)));
  return root;
}

// ---------- Setup guide ----------
const STEP_INFO = {
  hotkey: "Select your hotkey", phala: "Add your Phala hosting token", providers: "Add a provider API key",
  init: "Create your miner profile", deploy: "Deploy your worker", activate: "Activate your keys",
  offers: "Choose models to serve", resume: "Start accepting work",
};
function stepDone(st) {
  if (st.id === "resume") return findCheck("Accepting new work")?.ok === true && value(S.health)?.ok === true;
  if (st.id === "offers") {
    const bid = findCheck("Current model bid");
    return bid ? bid.ok : (value(S.health)?.checks || []).some((c) => (c.name || "").startsWith("Model: ") && checkState(c) === "PASS");
  }
  return !!st.done;
}
function pageSetup() {
  const o = S.overview, steps = o?.steps || [];
  const firstOpen = steps.find((x) => !stepDone(x))?.id;
  const open = S.openStep || firstOpen;
  const done = steps.filter(stepDone).length;
  const root = h("div", null, h("h1", null, "Setup guide"),
    h("p", { class: "lede" }, "Set up at your pace. Review costs and confirm before deploying or accepting paid work."),
    bar((done / Math.max(1, steps.length)) * 100),
    h("p", { class: "small muted", "data-setup-progress": "" }, done + " of " + steps.length + " steps done"));
  const list = h("ol", { class: "steps" });
  steps.forEach((st, i) => {
    const isDone = stepDone(st), isOpen = open === st.id;
    const body = isOpen ? h("div", { class: "body" }, stepBody(st.id)) : null;
    list.append(h("li", { "data-step": st.id, class: "step " + (isDone ? "done" : "") + (st.id === firstOpen ? " current" : "") },
      h("div", { class: "num" }, isDone ? "✓" : String(i + 1)),
      h("div", { class: "box" },
        h("div", { class: "title", role: "button", tabindex: "0", onclick: () => { S.openStep = isOpen ? "__none" : st.id; render(); },
          onkeydown: (e) => { if (e.key === "Enter") { S.openStep = isOpen ? "__none" : st.id; render(); } } },
          h("h3", null, st.id === "hotkey" && o?.hotkeyAttached ? "Your hotkey" : STEP_INFO[st.id] || st.id),
          h("span", { class: "badge " + (isDone ? "ok" : st.id === firstOpen ? "info" : "") }, isDone ? "DONE" : st.id === firstOpen ? "NEXT" : "")),
        body)));
  });
  root.id = "setup-guide";
  root.append(list);
  if (steps.length && done === steps.length) { const n = notice("ok", "Setup complete", "Your worker accepts work. Keep an eye on the Overview page; jobs depend on customer demand and your offers."); n.id = "setup-complete"; root.append(n); }
  return root;
}
function patchSetupProgress() {
  if (currentRoute() !== "setup") return;
  const steps = S.overview?.steps || [], done = steps.filter(stepDone).length, first = steps.find(st => !stepDone(st))?.id;
  const counter = document.querySelector("[data-setup-progress]");
  if (counter) counter.textContent = done + " of " + steps.length + " steps done";
  const fill = document.querySelector("#main .progress span");
  if (fill) fill.style.width = (100 * done / Math.max(1, steps.length)) + "%";
  for (const st of steps) {
    const row = document.querySelector('[data-step="' + st.id + '"]'); if (!row) continue;
    const complete = stepDone(st);
    row.classList.toggle("done", complete); row.classList.toggle("current", st.id === first);
    row.querySelector(".num").textContent = complete ? "✓" : String(steps.indexOf(st) + 1);
    const badge = row.querySelector(".title .badge"); badge.className = "badge " + (complete ? "ok" : st.id === first ? "info" : ""); badge.textContent = complete ? "DONE" : st.id === first ? "NEXT" : "";
  }
  const complete = steps.length > 0 && done === steps.length;
  if (!complete) $("#setup-complete")?.remove();
  else if (!$("#setup-complete")) { const n = notice("ok", "Setup complete", "Your worker accepts work. Keep an eye on the Overview page; jobs depend on customer demand and your offers."); n.id = "setup-complete"; $("#setup-guide")?.append(n); }
}
async function refreshAll() {
  await loadOverview();
  if (!managedHere() && !S.removal) await loadWorkloads();
  if (S.overview?.initialized) await loadHealth();
  if (currentRoute() === "worker") await loadWorkerAuto();
  render();
}
function stepBody(id) {
  const o = S.overview;
  switch (id) {
    case "hotkey": return stepHotkey(o);
    case "phala": return h("div", null,
      h("p", null, "Your worker runs in a sealed, verifiable machine (a TEE) on Phala Cloud. Phala bills your own account for hosting, usually a few cents per hour."),
      h("ol", { class: "small" },
        h("li", null, "Sign in at ", h("a", { href: "https://cloud.phala.network", target: "_blank", rel: "noopener noreferrer" }, "cloud.phala.network"), "."),
        h("li", null, "Open Settings → API Tokens (not \"Private AI\") and create a token."),
        h("li", null, "Paste it below. It is saved only on this computer, in an owner-only file.")),
      keyForm("phala", refreshAll, "phala"),
      continueButton("phala"));
    case "providers": return h("div", null,
      h("p", null, "The worker generates with your own provider account, so each job is billed there. Add at least one key. You can add more later on the API keys page."),
      ["fal", "vivix", "openrouter", "pika"].map((p) => keyRow(p, "providers")),
      h("details", { open: setupOtherProviders().some((p) => S.keyStatus[p]) },
        h("summary", null, "Other providers"), setupOtherProviders().map((p) => keyRow(p, "providers"))),
      continueButton("providers"));
    case "init": return h("div", null,
      h("p", null, "everycli proves you own the hotkey by signing a challenge on this computer, then downloads your signed worker configuration. No machine is created and nothing is paid."),
      S.hotkeySel ? h("p", { class: "small muted" }, "Using " + describeHotkey(S.hotkeySel) + ".") : o?.registration ? h("p", { class: "small muted" }, "Using the hotkey checked in step 1.") : null,
      button("Create miner profile", async () => {
        const job = await run("init", S.hotkeySel || {});
        if (job.state === "done") S.openStep = null;
        await refreshAll();
      }, "primary"));
    case "deploy": return stepDeploy(o);
    case "activate": {
      const attest = findCheck("Workload attestation");
      return h("div", null,
        h("p", null, "After the worker starts, it proves it's running the genuine, unmodified code (attestation). This usually takes a few minutes. Then your provider keys are released to it, sealed so only that worker can read them."),
        attest ? notice(attest.ok ? "ok" : "warn", attest.ok ? "Attestation verified" : "Waiting for attestation", attest.detail) : null,
        h("div", { class: "row" },
          button("Check again", async () => { await loadHealth(false); render(); }),
          button("Activate keys", async () => { await run("activate"); await refreshAll(); }, "primary")));
    }
    case "offers": return h("div", null,
      h("p", null, "Choose which models your worker serves and an optional discount. Jobs are routed to miners with active offers; a bigger discount can win more jobs but lowers your reward per job."),
      h("a", { class: "btn primary", href: "#/models" }, "Open Models"));
    case "resume": return h("div", null,
      h("p", null, "Resume lets the network route paid jobs to your worker. Your provider account pays for each generation; rewards are alpha emissions for accepted, verified work. Traffic isn't guaranteed."),
      button("Start accepting work", async () => { await run("resume"); await refreshAll(); }, "primary"));
  }
  return null;
}
function continueButton(id) {
  const st = (S.overview?.steps || []).find((x) => x.id === id);
  if (!st || !stepDone(st)) return null;
  return h("div", { class: "row end" }, h("button", { class: "btn primary", onclick: () => {
    S.openStep = null; render();
  } }, "Continue to next step"));
}
function describeHotkey(sel) {
  if (sel.wallet) return "wallet " + sel.wallet + " / hotkey " + (sel.hotkey || "default");
  if (sel.hotkeyFile) return "hotkey file " + sel.hotkeyFile;
  return "address " + sel.hotkeySs58;
}
function walletProblem(code) {
  if (code === "insecure_file" || code === "file_permission_denied") return "file permissions too open (run chmod 600 on it)";
  if (code === "hotkey_file_unreadable_use_hotkey_ss58") return "encrypted or unreadable; use Address only";
  return "unreadable (" + (code || "unknown") + ")";
}
function stepHotkey(o) {
  const wallets = S.wallets?.wallets || [];
  const options = [];
  for (const w of wallets) for (const k of w.hotkeys || []) options.push({ wallet: w.name, hotkey: k.name, address: k.address, error: k.error });
  const sel = h("select", { "aria-label": "Hotkey" }, options.length
    ? options.map((x, i) => h("option", { value: String(i) }, x.wallet + " / " + x.hotkey + (x.address ? "  ·  " + x.address.slice(0, 8) + "…" : "  ·  " + walletProblem(x.error))))
    : h("option", { value: "" }, "No wallets found in ~/.bittensor/wallets"));
  const attached = o?.profile?.hotkey || o?.registration?.hotkey;
  const registration = o?.registration;
  const uid = registration?.registered === true && registration.hotkey === attached && registration.network === o?.network
    && Number.isInteger(registration.uid) && registration.uid >= 0 && registration.uid <= 65535 ? registration.uid : null;
  const match = options.findIndex(x => x.address === attached);
  if (match >= 0) sel.value = String(match);
  const file = h("input", { type: "text", placeholder: "/path/to/hotkey", value: S.hotkeySel?.hotkeyFile || "" });
  const ss58 = h("input", { type: "text", placeholder: "5F… public address", value: attached || "" });
  let mode = attached ? "attached" : options.length ? "wallet" : "file";
  const panes = { attached: notice("ok", o?.hotkeyAttached ? "Attached hotkey" : "Selected hotkey", h("div", null,
    h("code", null, attached), h("span", { class: "pill" }, uid == null ? "UID not checked" : "UID " + uid),
    h("p", { class: "small" }, o?.hotkeyAttached ? "Attached on this computer. Deployment refreshes authorization automatically when needed." : "Select its local hotkey file if this computer has not attached it yet."))),
    wallet: field("Wallet and hotkey on this computer", sel), file: field("Hotkey file", file, "An unencrypted sr25519 hotkey file, as created by btcli."), address: field("Public hotkey address", ss58, "Checks registration only. Creating the profile later needs the hotkey file on this computer.") };
  const holder = h("div", null, panes[mode]);
  const seg = h("div", { class: "seg" }, [...(attached ? ["attached"] : []), "wallet", "file", "address"].map((m) => {
    const b = h("button", { class: m === mode ? "on" : "" }, { attached: "My hotkey", wallet: "My wallets", file: "Hotkey file", address: "Address only" }[m]);
    b.addEventListener("click", () => { mode = m; [...seg.children].forEach((c) => c.classList.toggle("on", c === b)); holder.replaceChildren(panes[m]); });
    return b;
  }));
  const result = h("div");
  if (S.registration) result.append(registrationView(S.registration));
  return h("div", { class: "stack" },
    h("p", null, "Your hotkey must be registered on SN" + (S.session?.netuid ?? 117) + " before you can mine. This check only reads the chain. everycli never registers, signs a transaction or pays a fee."),
    seg, holder,
    h("div", { class: "row" }, button("Check registration", async () => {
      let params;
      if (mode === "attached") params = { hotkeySs58: attached };
      else if (mode === "wallet") { const x = options[Number(sel.value)]; if (!x) return; params = { wallet: x.wallet, hotkey: x.hotkey }; }
      else if (mode === "file") params = { hotkeyFile: file.value };
      else params = { hotkeySs58: ss58.value };
      const job = await run("register-hotkey", params);
      S.registration = job;
      if (value(job)?.registered === true && ["wallet", "file"].includes(mode)) S.hotkeySel = params;
      await loadOverview(); render();
    }, "primary")),
    result);
}
function registrationView(job) {
  const v = value(job);
  if (errorOf(job)) return notice(job.state === "unknown" ? "warn" : "bad", job.state === "unknown" ? "Check not confirmed" : "Check failed", errorView(errorOf(job)));
  if (!v) return null;
  if (v.registered === true) return notice("ok", "Registered on SN" + (v.netuid ?? ""), (v.hotkey || "") + (v.uid != null ? " · UID " + v.uid : ""));
  return notice("warn", "Not registered yet", h("div", null,
    h("p", null, "Register this hotkey with the wallet that owns it (this costs TAO), then check again:"),
    v.registrationCommand ? copyBox(v.registrationCommand) : null));
}
function stepDeploy(o) {
  const limit = computeLimitInput("Maximum compute USD per hour");
  if (S.removal) return removalView();
  if (S.workloads?.length) return h("div", { class: "stack" }, workloadsNotice(limit),
    field("Maximum compute price for the new worker (USD per hour)", limit, "Used by \"Delete and deploy again\"."));
  if (o?.deployment?.needsReconcile) {
    return h("div", null, notice("warn", "An earlier deploy didn't finish cleanly", "Re-sync it first. Never deploy a second worker; it would bill twice."),
      button("Re-sync operation", async () => { await run("reconcile"); await refreshAll(); }, "primary"));
  }
  return h("div", { class: "stack" },
    h("p", null, "This creates your worker on Phala Cloud. Before anything is created you'll see the live hourly price and confirm it. Storage costs a little extra, and there is no automatic total spending cap: stop the worker from the Worker page when you're done."),
    field("Maximum compute price you accept (USD per hour)", limit, "Deploy refuses if Phala's current price is above this."),
    h("div", { class: "row" }, button("Review price and deploy", async () => {
      const job = await run("deploy", { maxHourlyUsd: limit.value });
      if (errorOf(job)?.error === "existing_cloud_workload") await loadWorkloads();
      await refreshAll();
    }, "primary")));
}

// ---------- workers this computer doesn't manage, delete and redeploy ----------
function managedHere() {
  const d = S.overview?.deployment || {};
  return !!d.appId && d.phase !== "deleted";
}
async function loadWorkloads() {
  if (!S.overview?.initialized || managedHere()) { S.workloads = null; return; }
  if (!(S.overview?.keys || []).some((k) => k.provider === "phala" && k.saved)) { S.workloads = null; return; }
  const job = await run("status", {}, { quiet: true });
  const v = value(job);
  S.workloads = Array.isArray(v?.cloudWorkloads) ? v.cloudWorkloads : null;
}
const shortId = (id) => id ? String(id).slice(0, 10) + "…" + String(id).slice(-4) : "—";
function workloadsNotice(limit) {
  const list = S.workloads || [];
  const ownLimit = !limit;
  if (ownLimit) limit = computeLimitInput("Maximum compute USD per hour for the new worker");
  return notice("warn", list.length === 1 ? "A worker for this miner already exists in your Phala account" : list.length + " workers for this miner already exist in your Phala account", h("div", { class: "stack" },
    h("p", null, "It was deployed from another computer or an earlier setup, so this computer can't manage it. Keep running it from where you set it up, or delete it here and deploy a new worker that this computer manages."),
    h("p", { class: "small" }, "Deleting is permanent: the VM and its storage are removed, its billing stops, and any job in progress is lost."),
    h("ul", { class: "checks" }, list.map((w) => h("li", { class: "action-row" }, h("span"),
      h("div", { class: "row" },
        h("div", { class: "grow" }, h("div", { class: "name mono", title: w.appId }, "VM " + shortId(w.appId)),
          h("div", { class: "detail" }, "Phala status: " + (w.status || "unknown") + (w.sameRelease ? "" : " · older or different release"))),
        button("Delete", async () => { await removeWorker(w.appId, false, null); }, "danger"),
        button("Delete and deploy again", async () => { await removeWorker(w.appId, false, limit.value); }, "primary"))))),
    ownLimit ? h("div", { class: "row" }, h("span", { class: "small" }, "New worker's maximum compute price"), limit, h("span", { class: "small muted" }, "USD/h, storage extra")) : null));
}
// Deletes a worker, waits until Phala has removed it, then optionally deploys a new one.
async function removeWorker(appId, recorded, redeployLimit) {
  const job = await run("delete", { appId }, { label: redeployLimit ? "Delete and deploy again" : "Delete worker" });
  if (job.state !== "done") return;
  S.removal = { appId, redeploy: redeployLimit, started: Date.now(), note: "Waiting for Phala to remove the VM…" };
  render();
  let gone = value(job)?.phase === "deleted";
  for (let i = 0; i < 60 && !gone; i++) {
    await new Promise((ok) => setTimeout(ok, 5000));
    const check = await run("delete", { appId }, { quiet: true });
    gone = value(check)?.phase === "deleted";
    if (check.state !== "done") break;
    if (S.removal) { S.removal.note = "Waiting for Phala to remove the VM… (" + Math.round((Date.now() - S.removal.started) / 1000) + " s)"; renderRemoval(); }
  }
  const redeploy = S.removal?.redeploy;
  S.removal = null;
  await loadOverview();
  if (!gone) {
    Activity.add("Delete worker").finish({ state: "attention", result: { value: { message: "Deletion was requested, but its completion is not confirmed. Check again from the Worker page before deploying another VM." } } });
  } else if (redeploy) {
    const dep = await run("deploy", { maxHourlyUsd: redeploy });
    if (errorOf(dep)?.error === "existing_cloud_workload") await loadWorkloads();
  }
  await loadWorkloads();
  await refreshAll();
}
function removalView() {
  const r = S.removal;
  return h("div", { class: "removal" }, notice("warn", r.redeploy ? "Deleting the old worker, then deploying a new one" : "Deleting the worker",
    h("div", null, h("p", { class: "removal-note" }, r.note), h("p", { class: "small muted" }, "VM " + shortId(r.appId) + ". Keep this page open; closing it doesn't undo the deletion."))));
}
function renderRemoval() {
  const el = $(".removal-note");
  if (el && S.removal) el.textContent = S.removal.note;
}

// ---------- API keys ----------
const CHECK_REASONS = {
  key_check_unreachable: "couldn't reach the provider right now",
  key_check_permission: "this key does not permit the read-only account check; generation access is unverified",
  key_check_inconclusive: "the provider gave an unexpected answer",
  key_check_unsupported: "no free check is available for this provider",
};
function keyStatusView(provider) {
  const st = S.keyStatus[provider];
  if (!st) return null;
  if (st.kind === "error") return h("div", { class: "key-status bad" }, errorView(st.error));
  return h("div", { class: "key-status " + st.kind }, st.text);
}
function checkMessage(provider, check, saving) {
  const name = providerName(provider);
  if (check?.result === "valid") return { kind: "ok", text: (saving ? "Saved. " : "") + name + " accepted this key." };
  check ||= { result: "unverified", reason: "key_check_inconclusive" };
  if (check.result === "rejected") return { kind: "bad", text: name + " rejected the saved key. Replace it with a working key." };
  return { kind: "warn", text: (saving ? "Saved, but not verified: " : "Not verified: ") + (CHECK_REASONS[check.reason] || check.reason) + ". It will be used as entered." };
}
function keyForm(provider, after, stayOn) {
  const saved = (S.overview?.keys || []).find((k) => k.provider === provider)?.saved;
  const input = h("input", { type: "password", autocomplete: "off", spellcheck: "false", placeholder: saved ? "Saved · paste a new key to replace" : "Paste key", "aria-label": providerName(provider) + " key" });
  const finish = async () => {
    if (stayOn) S.openStep = stayOn;
    await loadOverview();
    if (currentRoute() === "keys") { S.balances = null; await loadBalances(); }
    if (after) await after();
    else {
      const row = form.closest("[data-provider-key]");
      if (row?.tagName === "LI") row.replaceWith(providerKeyItem((S.overview?.keys || []).find(k => k.provider === provider)));
      else if (row) row.replaceWith(keyRow(provider, stayOn));
      else form.replaceWith(keyForm(provider, after, stayOn));
      patchSetupProgress(); renderMeta(); renderNav();
    }
  };
  const save = button(saved ? "Replace" : "Save", async () => {
    if (S.pendingOperations.size) {
      S.keyStatus[provider] = { kind: "warn", text: "Check the pending worker operation before changing keys." };
      status.replaceChildren(keyStatusView(provider)); return;
    }
    const key = input.value;
    if (!key.trim()) { S.keyStatus[provider] = { kind: "warn", text: "Paste a key first." }; return finish(); }
    input.value = "";
    S.keyStatus[provider] = { kind: "muted", text: "Checking the key with " + providerName(provider) + "…" };
    status.replaceChildren(keyStatusView(provider));
    const r = await api("/api/keys", { provider, key });
    const err = r.data.error;
    if (err?.error === "credential_rejected" && provider !== "fal-billing") err.message = providerName(provider) + " rejected this key, so it was not saved.";
    S.keyStatus[provider] = r.ok ? checkMessage(provider, r.data.check, true) : { kind: "error", error: err };
    await finish();
  }, "primary");
  const test = saved ? button("Test", async () => {
    S.keyStatus[provider] = { kind: "muted", text: "Checking the saved key…" };
    status.replaceChildren(keyStatusView(provider));
    const r = await api("/api/keys/check", { provider });
    S.keyStatus[provider] = r.ok ? checkMessage(provider, r.data.check, false) : { kind: "error", error: r.data.error };
    status.replaceChildren(keyStatusView(provider));
  }, "small") : null;
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") save.click(); });
  const status = h("div", { class: "key-status-wrap" }, keyStatusView(provider));
  const form = h("div", { class: "keyform" },
    h("div", { class: "row" }, h("span", { class: "badge " + (saved ? "ok" : "") }, saved ? "SAVED" : "NOT SAVED"), input, save, test),
    status);
  return form;
}
function setupOtherProviders() { return Object.keys(PROVIDERS).filter(p => !["phala", "fal", "vivix", "openrouter", "pika"].includes(p) && !p.endsWith("-billing")); }
function keyRow(provider, stayOn) {
  const [name, hint] = PROVIDERS[provider] || [provider, ""];
  return h("div", { class: "card flat", "data-provider-key": provider }, h("h3", null, name), h("p", { class: "small muted" }, hint), keyForm(provider, null, stayOn));
}
function providerKeyItem(k) {
  if (!k) return h("li");
  return h("li", { "data-provider-key": k.provider },
    h("div", null, h("b", null, providerName(k.provider)), h("div", { class: "small muted" }, (PROVIDERS[k.provider] || [, ""])[1])),
    h("div", { class: "row" }, keyForm(k.provider), k.saved ? button("Remove", async () => {
      const job = await run("remove-api-key", { provider: k.provider });
      if (job.state !== "done") return;
      await loadOverview(); await loadBalances();
      document.querySelector('[data-provider-key="' + k.provider + '"]')?.replaceWith(providerKeyItem((S.overview?.keys || []).find(row => row.provider === k.provider)));
      patchSetupProgress();
    }, "small danger") : null));
}
function pageKeys() {
  const o = S.overview, keys = o?.keys || [];
  const deployed = workerControls().apply;
  const groups = [["hosting", "Hosting"], ["generation", "Generation providers"], ["billing", "Balance checks (optional)"]];
  const root = h("div", null, h("h1", null, "API keys"),
    h("p", { class: "lede" }, "Keys are stored only on this computer, in an owner-only file, and sealed into your worker. This page never shows a saved key again. To revoke a leaked key, also revoke it at the provider."));
  if (deployed) root.append(notice("", "Changed a generation key?", h("div", null,
    h("p", null, "Your running worker keeps its current keys until you apply new ones. Applying pauses new work, restarts the worker with the new keys and waits for fresh attestation."),
    button("Apply keys to worker", async () => { await run("apply-api-keys"); await refreshAll(); }))));
  root.append(balancesCard());
  for (const [kind, title] of groups) {
    const rows = keys.filter((k) => k.kind === kind);
    root.append(card(title, null, h("ul", { class: "keylist" }, rows.map(providerKeyItem))));
  }
  return root;
}

// ---------- Models ----------
function readiness(r) {
  for (const status of [r.providerAvailability?.status, r.workerAvailability?.status]) {
    const labels = {removed:"Removed by provider",deprecated:"Deprecated by provider",access_denied:"Provider access unavailable for this key",temporarily_unavailable:"Availability check temporarily unavailable"};
    if (labels[status]) return [labels[status], status === "temporarily_unavailable" ? "warn" : "bad"];
  }
  const b = r.blockers || [];
  if (b.includes("pricing_required")) return ["Pricing required", "warn"];
  if (b.includes("worker_release_required")) return ["Worker update required", "warn"];
  if (b.includes("worker_offline")) return ["Worker offline", "bad"];
  if (b.includes("worker_capabilities_unknown")) return ["Worker check required", "warn"];
  if (b.includes("worker_provider_required")) return ["Add provider key to worker", "warn"];
  if (r.workerAvailability?.status === "unchecked" && r.offerable === true) return ["Available · account access not checked", "warn"];
  if (!S.overview?.initialized && r.catalogGroup === "supported") return ["Listed · setup required", ""];
  if (r.offerable === true) return ["Available", "ok"];
  return ["Not reported", ""];
}
function pageModels() {
  const root = h("div", null, h("h1", null, "Models"),
    h("p", { class: "lede" }, "Models the network can route to you, and your offer for each. The discount is taken off the base miner reward, not your provider's price; your provider still bills its normal rate."));
  const refresh = button("Refresh", async () => { await loadOffers(false); render(); });
  if (!S.offers && !S.offersError) { root.append(card("Offers", [refresh], h("p", { class: "muted" }, "Loading…"))); loadOffers().then(render); return root; }
  if (S.offersError) { root.append(card("Offers", [refresh], errorView(S.offersError))); return root; }
  const filters = [["ready", "Ready for my keys"], ["active", "My active offers"], ["all", "All models"], ["candidate", "Candidates / retired"]];
  const seg = h("div", { class: "seg" }, filters.map(([f, label]) => h("button", { disabled: S.batchBusy, class: S.offerFilter === f ? "on" : "", onclick: () => { S.offerFilter = f; S.selectedOffers.clear(); clearBatchReport(); if (f === "all") acknowledgeModels(); render(); } }, label,
    f === "all" && S.newModels.size ? h("span", { class: "badge info", "aria-label": S.newModels.size + " new models" }, S.newModels.size + " new") : null)));
  const search = h("input", { type: "search", disabled: S.batchBusy, value: S.modelQuery, placeholder: "Search models", "aria-label": "Search models", oninput: e => { S.modelQuery = e.target.value; patchModels(); } });
  const providers = [...new Set((S.offers.offers || []).map(r => r.provider).filter(Boolean))].sort();
  const provider = h("select", { "aria-label": "Filter model provider", disabled: S.batchBusy, onchange: e => { S.modelProvider = e.target.value; patchModels(); } },
    h("option", { value: "" }, "All providers"), providers.map(p => h("option", { value: p }, providerName(p)))); provider.value = S.modelProvider;
  const kind = h("select", { "aria-label": "Filter model type", disabled: S.batchBusy, onchange: e => { S.modelKind = e.target.value; patchModels(); } },
    h("option", { value: "" }, "All types"), ["video", "image", "audio"].map(v => h("option", { value: v }, v[0].toUpperCase() + v.slice(1)))); kind.value = S.modelKind;
  if (S.offerFilter === "candidate") root.append(notice("", "Not available for new offers", "These contracts are unpriced or retired. Existing offers can still be withdrawn."));
  const body = h("div", { id: "model-list" });
  root.append(card("Offers", [seg, refresh], h("div", { class: "row model-filters" }, search, provider, kind), body));
  fillModels(body);
  return root;
}
function visibleOffers() {
  let rows = (S.offers?.offers || []).filter(integratedModel);
  if (S.overview?.initialized && S.offerFilter === "active") rows = rows.filter(r => r.offered);
  if (S.overview?.initialized && S.offerFilter === "ready") rows = rows.filter(r => canApplyOffer(r) && readiness(r)[0].startsWith("Available") && r.localKeyConfigured !== false);
  if (S.offerFilter === "all" || (!S.overview?.initialized && S.offerFilter === "ready")) rows = rows.filter(r => r.catalogGroup !== "candidate" && r.providerAvailability?.status !== "removed");
  if (S.offerFilter === "candidate") rows = rows.filter(r => r.catalogGroup === "candidate" || ["removed", "deprecated"].includes(r.providerAvailability?.status));
  const q = S.modelQuery.trim().toLowerCase();
  return rows.filter(r => (!S.modelProvider || r.provider === S.modelProvider) && (!S.modelKind || (r.kind || r.contract?.kind) === S.modelKind)
    && (!q || [r.name, r.model, r.providerApiId, providerName(r.provider)].filter(Boolean).join(" ").toLowerCase().includes(q)));
}
function fillModels(body) {
  const rows = visibleOffers();
  body.replaceChildren(h("p", { class: "small muted" }, rows.length + " models shown"),
    S.overview?.initialized ? batchToolbar(rows) : h("span"),
    S.batchReport ? batchReportView() : h("span"),
    rows.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm" },
      h("thead", null, h("tr", null, ["Model", "Provider", "Readiness", "Your offer", "Base reward", "Change"].map(t => h("th", null, t)))),
      h("tbody", null, rows.map(offerRow)))) : h("p", { class: "empty" }, "No models match these filters. Try All models or clear the search."),
    h("p", { class: "small muted" }, "Base reward is a reference value per job; actual jobs are repriced for their settings. Offers do not guarantee traffic or profit."));
  if (S.batchBusy) body.querySelectorAll("button, input").forEach(el => { el.disabled = true; });
}
function patchModels() { const body = $("#model-list"); if (body) fillModels(body); }
function offerBlocker(r) {
  if (!integratedModel(r)) return "This contract is not supported.";
  const provider = r.providerAvailability?.status;
  if (provider === "removed" || provider === "deprecated") return "This model was " + provider + " by the provider.";
  if (r.authorized === false || (r.blockers || []).includes("worker_release_required")) return "Update the worker to enable this model.";
  if (r.pricingAvailable === false || (r.blockers || []).includes("pricing_required") || r.catalogGroup === "candidate") return "There is no current price for this model.";
  // Declaring a bid is different from being ready to execute a job. A worker
  // can be offline, or its account check can be transiently unavailable, while
  // the coordinator still authorizes a priced offer. The CLI rechecks the
  // signed price/revision and the coordinator remains the authority.
  if (r.authorized === true && r.pricingAvailable === true) return null;
  return r.offerable === false ? "This model is not available for new offers." : null;
}
function canApplyOffer(r) { return offerBlocker(r) === null; }
function selectableOffer(r) { return integratedModel(r) && (r.offered || (canApplyOffer(r) && r.localKeyConfigured !== false)); }
function batchToolbar(rows) {
  const eligible = rows.filter(selectableOffer);
  const visible = new Set(eligible.map(r => r.model));
  for (const id of S.selectedOffers) if (!visible.has(id)) S.selectedOffers.delete(id);
  const count = S.selectedOffers.size;
  const select = h("input", { type: "checkbox", checked: count > 0 && count === eligible.length, disabled: S.batchBusy || !eligible.length,
    "aria-label": "Select all editable models in this tab", onchange: e => { S.selectedOffers = new Set(e.target.checked ? eligible.map(r => r.model) : []); patchModels(); } });
  select.indeterminate = count > 0 && count < eligible.length;
  const discount = h("input", { type: "number", min: "0", max: "99.9", step: "0.01", value: S.batchDiscount, class: "tiny", "aria-label": "Batch discount percent", oninput: e => { S.batchDiscount = e.target.value; } });
  const apply = button("Apply discount", () => batchOffers(rows, false), "primary small");
  const withdraw = button("Withdraw selected", () => batchOffers(rows, true), "small");
  const selected = rows.filter(r => S.selectedOffers.has(r.model));
  const applicable = selected.filter(canApplyOffer).length, withdrawable = selected.filter(r => r.offered).length;
  apply.disabled = S.batchBusy || !applicable;
  withdraw.disabled = S.batchBusy || !withdrawable;
  discount.disabled = S.batchBusy;
  return h("div", { class: "batch-toolbar stack" }, h("div", { class: "row" },
    h("label", { class: "row" }, select, "Select all in this tab"), h("b", null, count + " selected"),
    count && applicable !== count ? h("span", { class: "small muted" }, applicable + " can receive a discount; " + (count - applicable) + " withdraw only") : null,
    discount, h("span", null, "% off"), apply, withdraw),
    h("p", { class: "small muted" }, S.batchBusy ? "Preparing or applying your batch…" : "Select models, review once, and apply together. If pricing changes, nothing is applied."));
}
function clearBatchReport() {
  clearTimeout(S.batchReportTimer); S.batchReportTimer = null; S.batchReport = null;
  document.querySelectorAll("[data-batch-report]").forEach(el => el.remove());
}
function showBatchReport(report, epoch = S.navigationEpoch) {
  if (epoch !== S.navigationEpoch || currentRoute() !== "models") return;
  clearBatchReport(); S.batchReport = report;
  // Remove only the transient report. Do not rebuild model forms and lose drafts.
  S.batchReportTimer = setTimeout(clearBatchReport, report.some(r => !r.ok) ? 30000 : 12000);
}
function batchReportView() {
  const report = S.batchReport;
  const view = notice(report.some(r => !r.ok) ? "warn" : "ok", "Batch results", h("div", null,
    button("Dismiss", clearBatchReport, "small"),
    h("ul", null, report.map(r => h("li", null, r.model + ": " + r.message)))));
  view.setAttribute("data-batch-report", ""); return view;
}
async function batchOffers(rows, withdraw) {
  if (S.batchBusy) return;
  const latest = new Map((S.offers?.offers || []).map(r => [r.model, r]));
  const selected = rows.filter(r => S.selectedOffers.has(r.model)).map(r => latest.get(r.model) || r);
  if (!selected.length) return;
  const discount = S.batchDiscount.trim();
  if (!withdraw && (!/^(?:0|[1-9]\d?)(?:\.\d{1,2})?$/.test(discount) || Number(discount) > 99.9)) {
    showBatchReport([{ model: "Discount", ok: false, message: "Enter 0–99.9%, with at most two decimal places." }]); render(); return;
  }
  const epoch = S.navigationEpoch;
  S.batchBusy = true; clearBatchReport(); render();
  const report = [], changes = selected.filter(r => withdraw ? r.offered : canApplyOffer(r));
  for (const row of selected) {
    if (withdraw && !row.offered) report.push({ model: row.name || row.model, ok: true, message: "Already withdrawn; unchanged." });
    else if (!withdraw && !canApplyOffer(row)) report.push({ model: row.name || row.model, ok: false, skipped: true,
      message: "Skipped: " + offerBlocker(row) + (row.offered ? " Your existing offer was not changed; it can still be withdrawn." : " No offer was created.") });
  }
  try {
    if (changes.length) {
      const params = { models: changes.map(r => r.model).join(","), ...(withdraw ? { withdraw: true } : { discountPct: discount }) };
      const prepared = await run("offer-batch", params, { quiet: true, prepareOnly: true });
      let job = prepared;
      if (prepared.state === "confirm") {
        const yes = epoch === S.navigationEpoch && await confirmDialog(prepared.result.question, "Batch model offers");
        job = yes && epoch === S.navigationEpoch
          ? await run("offer-batch", params, { quiet: true, approvals: [prepared.result.hash], exactApproval: true })
          : { state: "cancelled" };
      }
      const ok = job.state === "done";
      const message = ok ? (withdraw ? "Withdrawn." : `Discount set to ${discount}%.`)
        : job.state === "confirm" ? "Quote changed; nothing applied. Review again."
        : job.state === "cancelled" ? "Cancelled; unchanged."
        : errorOf(job)?.message || "Result not confirmed. Refresh offers before retrying.";
      for (const r of changes) { report.push({ model: r.model, ok, message }); if (ok) { S.selectedOffers.delete(r.model); S.offerDrafts.delete(r.model); } }
    }
  } catch { report.push({ model: "Batch", ok: false, message: "Result not confirmed. Refresh offers before retrying." });
  } finally {
    showBatchReport(report, epoch); S.batchBusy = false;
    await loadOffers();
    if (epoch === S.navigationEpoch && currentRoute() === "models") render(); else renderNav();
  }
}
function offerRow(r) {
  const [ready, tone] = readiness(r);
  const disc = h("input", { type: "number", min: "0", max: "99.9", step: "0.01", value: S.offerDrafts.get(r.model) ?? (r.offered && r.discountBp != null ? String(r.discountBp / 100) : "0"), oninput: e => S.offerDrafts.set(r.model, e.target.value), class: "tiny", "aria-label": "Discount percent for " + (r.name || r.model) });
  const baseMicrousd = r.baseMinerRewardMicrousd ?? r.pricing?.minerRewardMicrousd;
  const base = typeof baseMicrousd === "number" ? baseMicrousd / 1e6 : null;
  const yours = r.offered ? (r.pricingCurrent ? "Active · " + ((r.discountBp || 0) / 100).toFixed(2) + "% off" : "Stale pricing") : "Not offered";
  const apply = button(r.offered ? "Update" : "Offer", async () => { const job = await run("offer", { model: r.model, discountPct: disc.value }); if (job.state === "done") S.offerDrafts.delete(r.model); await loadOffers(); await loadHealth(); render(); }, "small primary");
  apply.disabled = !canApplyOffer(r); disc.disabled = !canApplyOffer(r);
  return h("tr", null,
    h("td", { "data-label": "Model" }, h("label", { class: "row" }, S.overview?.initialized ? h("input", { type: "checkbox", checked: S.selectedOffers.has(r.model), disabled: S.batchBusy || !selectableOffer(r), "aria-label": "Select " + (r.name || r.model), onchange: e => { if (e.target.checked) S.selectedOffers.add(r.model); else S.selectedOffers.delete(r.model); patchModels(); } }) : null, h("b", null, r.name || r.model)), h("details", { class: "model-details" }, h("summary", null, "Model details"), h("span", { class: "id" }, "Everyframe contract: " + r.model), h("span", { class: "id" }, "Provider API ID: " + (r.providerApiId || "Not reported")))),
    h("td", { "data-label": "Provider" }, providerName(r.provider || ""), r.localKeyConfigured === false ? h("div", { class: "small muted" }, "No key saved") : null),
    h("td", { "data-label": "Readiness" }, h("span", { class: "badge " + tone }, ready)),
    h("td", { "data-label": "Your offer" }, h("span", { class: "badge " + (r.offered ? (r.pricingCurrent ? "ok" : "warn") : "") }, yours)),
    h("td", { "data-label": "Base reward" }, base != null ? fmtUsd(base, base < 1 ? 3 : 2) : "—"),
    h("td", { "data-label": "Change" }, !S.overview?.initialized ? h("a", { href: "#/setup" }, "Set up to offer") : h("div", { class: "row nowrap" }, disc, h("span", { class: "small muted" }, "% off"),
      apply,
      r.offered ? button("Withdraw", async () => { await run("offer", { model: r.model, withdraw: true }); await loadOffers(); await loadHealth(); render(); }, "small") : null)));
}

// ---------- Worker ----------
function computeLimitInput(label) {
  return h("input", { type: "number", min: "0.01", step: "0.01", value: S.computeLimit, oninput: e => { S.computeLimit = e.target.value; }, class: "narrow", "aria-label": label });
}
function workerStateSummary() {
  const [phase, tone] = workerState(), d = S.overview?.deployment || {};
  const card = stat("State", phase, d.appId ? "App " + String(d.appId).slice(0, 10) + "…" : "", tone);
  card.id = "worker-state-summary"; return card;
}
function patchWorkerView() {
  if (currentRoute() !== "worker") return;
  $("#worker-state-summary")?.replaceWith(workerStateSummary());
  const c = workerControls(), enabled = {
    "Apply keys": c.apply, "Shut down": c.shutdown, "Pause": c.pause, "Resume": c.resume, "Start": c.start,
    "Emergency shutdown": c.exists, "Delete": c.exists, "Check deletion": c.exists,
    "Delete and deploy again": c.exists && !c.pending, "Check": managedHere(),
    "Update": managedHere() && !c.pending && automaticMode() !== true,
  };
  for (const button of document.querySelectorAll("#main button")) {
    if (Object.hasOwn(enabled, button.textContent) && button.dataset.busy !== "true") button.disabled = !enabled[button.textContent];
  }
}
async function loadWorkerAuto() {
  if (!S.overview?.initialized || S.workerAutoBusy) return;
  S.workerAutoBusy = true;
  try { S.workerAuto = await run("worker-auto-status", {}, { quiet: true }); }
  finally { S.workerAutoBusy = false; }
  if (currentRoute() === "worker") {
    $("#worker-auto-status")?.replaceWith(workerAutoCard());
    patchWorkerView();
  }
}
function automaticMode() {
  const v = value(S.workerAuto);
  return v ? (v.automaticUpdates?.enabled === true || v.enabled === true) : null;
}
function workerAutoCard() {
  const v = value(S.workerAuto), a = v?.automaticUpdates || v, enabled = automaticMode();
  const labels = { idle: "Idle", updated: "Updated", current: "Up to date", needs_attention: "Needs attention", failed: "Needs attention", updating: "Updating", enrolling: "Enrolling" };
  const enable = button("Enable…", async () => { await run("worker-auto-enable"); await loadWorkerAuto(); await refreshAll(); }, "primary");
  const disable = button("Disable", async () => { await run("worker-auto-disable"); await loadWorkerAuto(); await refreshAll(); });
  enable.disabled = enabled !== false || !workerControls().exists; disable.disabled = enabled !== true;
  const panel = card("Automatic worker updates", null,
    errorOf(S.workerAuto) ? errorView(errorOf(S.workerAuto)) : h("dl", { class: "kv" },
      h("dt", null, "Mode"), h("dd", null, enabled === null ? "Not checked" : enabled ? "Automatic (in VM)" : "Manual"),
      h("dt", null, "Last result"), h("dd", null, labels[a?.report?.state || a?.state] || a?.report?.state || "Not reported"),
      h("dt", null, "Installed release"), h("dd", null, S.overview?.profile?.release || "Not reported")),
    h("p", { class: "small muted" }, "Manual updates remain the default. Automatic updates run in an isolated controller inside your VM, including while this computer is offline. Manual provider migrations require disabling automation first, then updating and re-enabling it."),
    h("div", { class: "row" }, button("Check status", loadWorkerAuto), enable, disable));
  panel.id = "worker-auto-status"; return panel;
}
function updateSummary(job, worker = false) {
  if (errorOf(job)) return errorView(errorOf(job));
  const v = value(job); if (!v) return h("p", { class: "muted" }, "Not checked");
  const available = v.state === "available" || v.updateAvailable === true;
  const current = v.state === "current" || v.updateAvailable === false;
  const rows = worker ? [["Installed release", S.overview?.profile?.release], ["Available release", typeof v.release === "string" ? v.release : v.newRelease]]
    : [["Installed version", v.installedVersion || v.currentVersion || S.session?.version], ["Latest version", v.latestVersion || v.latest]];
  return h("div", { class: "stack" }, h("b", null, available ? "Update available" : current ? "Up to date" : v.state === "unavailable" ? "No compatible update" : "Update check finished"),
    h("dl", { class: "kv" }, rows.filter(([,v]) => typeof v === "string").flatMap(([k,v]) => [h("dt", null, k), h("dd", { class: "wrap" }, v)])),
    v.next ? h("p", { class: "small muted" }, v.next) : null);
}
function pageWorker() {
  const o = S.overview, d = o?.deployment || {}, p = o?.profile || {};
  const [phase, tone] = workerState();
  const root = h("div", null, h("h1", null, "Worker"),
    h("p", { class: "lede" }, "Manage the machine that does the work. Every action shows what it will do and asks before anything changes or costs money. Closing this page never stops your worker."));
  if (!o?.initialized) { root.append(notice("warn", "No miner profile yet", "Finish the setup guide first.")); return root; }
  if (S.removal) root.append(removalView());
  else if (S.workloads?.length) root.append(workloadsNotice(null));
  else if (!managedHere()) root.append(notice("", "No worker deployed from this computer", h("div", null, "Deploy one from step 5 of the ", h("a", { href: "#/setup" }, "setup guide"), ".")));
  root.append(h("div", { class: "grid" },
    workerStateSummary(),
    nameStat(),
    stat("Compute price", d.computeHourlyUsd ? "$" + d.computeHourlyUsd + "/h" : "—", "Storage extra")));
  if (d.needsReconcile) root.append(notice("warn", "Confirm worker state", h("div", null,
    h("p", null, "A lifecycle operation is pending. Re-sync checks its result at Phala and the coordinator without repeating the cloud operation."),
    button("Re-sync operation", async () => { await run("reconcile"); await refreshAll(); }, "primary"))));
  root.append(workerAutoCard());
  const ph = d.phase || "not_deployed", controls = workerControls();
  const when = (ok, el, why) => { if (!ok) { el.disabled = true; el.title = why; } return el; };
  const startLimit = computeLimitInput("Maximum compute USD per hour");
  const actions = [
    ["Reconnect profile", "Prove ownership with the same saved hotkey and refresh local authorization. Keeps keys and history; does not deploy or change the VM.", reconnectButton()],
    ["Re-sync state", "Confirm completed starts, shutdowns or interrupted operations on this same VM.",
      when(!!d.appId, button("Re-sync state", async () => { await run("reconcile"); await refreshAll(); }), "No deployment recorded.")],
    ["Start accepting work", "Lets the network route paid jobs to this worker again.",
      when(controls.resume,
        button("Resume", async () => { await run("resume"); await refreshAll(); }, "primary"), "Already accepting work, or the worker isn't ready.")],
    ["Pause new work", "Finishes current jobs, then stops taking new ones. The machine keeps running and billing.",
      when(controls.pause, button("Pause", async () => { await run("stop", { drainOnly: true }); await refreshAll(); }), "Only a running worker can be paused.")],
    ["Update worker", "Checks for a newer signed worker release. Updating pauses work, updates the same machine, waits for attestation and restores the previous state.",
      h("div", { class: "row" }, when(managedHere(), button("Check", async () => { S.workerUpdate = await run("worker-update-check"); render(); }), "No worker deployed from this computer."),
        when(managedHere() && !controls.pending && automaticMode() !== true, button("Update", async () => { await run("worker-update"); await loadWorkerAuto(); await refreshAll(); }, "primary"), automaticMode() === true ? "Disable automatic worker updates before a manual provider migration." : "No worker ready for an update."))],
    ["Apply saved keys", "Pushes keys you changed on the API keys page to this worker (pauses, restarts, re-attests).",
      when(controls.apply, button("Apply keys", async () => { await run("apply-api-keys"); await refreshAll(); }), "No running worker.")],
    ["Shut down", "Drains work, then shuts the machine down. Storage may still be billed; nothing is deleted.",
      when(controls.shutdown, button("Shut down", async () => { await run("stop"); await refreshAll(); }, "danger"), "No running worker.")],
    ["Emergency shutdown", "Use if the coordinator is unavailable. Can interrupt paid jobs; storage remains billable. The CLI still verifies the exact VM.",
      when(controls.exists, button("Emergency shutdown", async () => { await run("stop", { force: true }); await refreshAll(); }, "danger"), "No deployment recorded.")],
    ["Delete worker", "Permanently deletes this VM and its storage. Billing for it stops; jobs in progress are lost. You can deploy a new worker afterwards.",
      when(controls.exists, button(["deleting", "delete_intent"].includes(ph) ? "Check deletion" : "Delete", async () => { await removeWorker(d.appId, true, null); }, "danger"), "No worker recorded on this computer.")],
    ["Redeploy", "Deletes this worker, waits until Phala removes it, then deploys a fresh one at up to the price below.",
      when(controls.exists && !controls.pending, button("Delete and deploy again", async () => { await removeWorker(d.appId, true, startLimit.value); }, "danger"), "No worker recorded on this computer.")],
    ["Start a stopped worker", "Starts the same machine again at up to this compute price.", h("div", { class: "row" }, startLimit, h("span", { class: "small muted" }, "USD/h max"),
      when(controls.start, button("Start", async () => { await run("start", { maxHourlyUsd: startLimit.value }); await refreshAll(); }), "Only a stopped worker without an unresolved start can be started."))],
  ];
  const renderActions = rows => h("ul", { class: "checks" }, rows.map(([t, desc, ctl]) => h("li", { class: "action-row" },
    h("span"), h("div", { class: "row" }, h("div", { class: "grow" }, h("div", { class: "name" }, t), h("div", { class: "detail" }, desc)), ctl))));
  const advanced = new Set(["Emergency shutdown", "Delete worker", "Redeploy"]);
  root.append(card("Actions", null, renderActions(actions.filter(([t]) => !advanced.has(t)))));
  root.append(h("details", { class: "card" }, h("summary", null, "Advanced · emergency shutdown, deletion and redeploy"), renderActions(actions.filter(([t]) => advanced.has(t)))));
  root.append(card("Worker logs", [button("Read logs", async () => { await run("logs"); })],
    h("p", { class: "small muted" }, "Read bounded, sanitized worker events. Private logs may require the Phala console.")));
  if (S.workerUpdate) root.append(card("Worker update check", null, updateSummary(S.workerUpdate, true)));
  root.append(card("Profile", null, h("dl", { class: "kv" },
    h("dt", null, "Miner"), h("dd", { class: "mono" }, p.minerId || "—"),
    h("dt", null, "Hotkey"), h("dd", { class: "mono" }, p.hotkey || "—"),
    h("dt", null, "Network"), h("dd", null, (S.session?.network || "") + " · SN" + (S.session?.netuid || "")),
    h("dt", null, "Signed worker name"), h("dd", { class: "mono wrap" }, p.workerName || "—"),
    h("dt", null, "Profile folder"), h("dd", { class: "mono wrap" }, S.session?.stateDir || "—"))));
  root.append(card("everycli", [button("Check for update", async () => { S.cliUpdate = await run("cli-update-check"); render(); })],
    h("p", null, "Installed: everycli " + (S.session?.version || "")),
    S.cliUpdate ? updateSummary(S.cliUpdate) : null,
    h("p", { class: "small muted" }, "Install CLI updates in your terminal with everycli update, then restart everycli ui. CLI updates never change your worker.")));
  return root;
}

// ---------- Help ----------
function pageHelp() {
  const root = h("div", null, h("h1", null, "Help"), h("p", { class: "lede" }, "Answers to common questions, and what each error means."));
  root.append(card("Common questions", null, h("dl", { class: "kv" },
    h("dt", null, "Remote server?"), h("dd", null, "This page only listens on the server's own 127.0.0.1. On your computer run ", h("code", null, "ssh -N -L 47117:127.0.0.1:47117 <user>@<server>"), ", then open the link everycli ui printed."),
    h("dt", null, "Close the page?"), h("dd", null, "Closing the page or stopping everycli ui never stops your worker. Use the Worker page to pause or shut it down."),
    h("dt", null, "What does it cost?"), h("dd", null, "Phala bills hosting to your account (compute per hour, plus storage). Your provider bills each generation. Rewards are alpha emissions for accepted, verified work. Nothing is guaranteed."),
    h("dt", null, "Where are my keys?"), h("dd", null, "Only on this computer in your miner profile folder (owner-only files), and sealed inside your attested worker. They are never shown here again."),
    h("dt", null, "Terminal instead?"), h("dd", null, "Every action here is an everycli command; the terminal and this page share the same profile.")),
    h("p", { class: "small" }, h("a", { href: "https://docs.everyframe.studio/mining/", target: "_blank", rel: "noopener noreferrer" }, "Mining docs"), " · ",
      h("a", { href: "https://github.com/everyframe-studio/everyframe-miner", target: "_blank", rel: "noopener noreferrer" }, "everyframe-miner on GitHub"))));
  const q = h("input", { type: "text", placeholder: "Search error codes or messages", "aria-label": "Search errors" });
  const list = h("ul", { class: "checks" });
  const draw = () => {
    const term = q.value.trim().toLowerCase();
    const codes = (S.help?.codes || []).filter((c) => !term || (c.code + " " + c.message + " " + c.next).toLowerCase().includes(term));
    list.replaceChildren(...codes.slice(0, 80).map((c) => h("li", null, h("span", { class: "badge info" }, "CODE"),
      h("div", null, h("div", { class: "name mono" }, c.code), h("div", null, c.message), h("div", { class: "detail" }, c.next)))));
    if (!codes.length) list.append(h("li", null, h("span"), h("div", { class: "muted" }, S.help ? "No match." : "Loading…")));
  };
  q.addEventListener("input", draw);
  if (!S.help) api("/api/help").then((r) => { if (r.ok) { S.help = r.data; draw(); } });
  draw();
  root.append(card("Error codes", [q], list));
  return root;
}

// ---------- boot ----------
async function pairFromLink() {
  const m = location.hash.match(/pair=([0-9a-f]{32})/);
  if (!m) return;
  history.replaceState(null, "", location.pathname);
  const r = await fetch("/api/pair", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code: m[1] }) }).catch(() => null);
  const data = r ? await r.json().catch(() => ({})) : {};
  if (r && r.ok && data.token) {
    S.token = data.token;
    try { sessionStorage.setItem("everycli-token", data.token); } catch { /* */ }
  } else if (!S.token) {
    $("#main").replaceChildren(notice("bad", "This link was already used", "Open the newest link printed by everycli ui in your terminal."));
  }
}
async function boot() {
  await pairFromLink();
  if (!S.token) { if (!$("#main .notice")) renderUnpaired(); return; }
  const s = await api("/api/session");
  if (!s.ok) return;
  S.session = s.data;
  restorePendingOperations();
  await loadOverview();
  const w = await api("/api/wallets"); if (w.ok) S.wallets = w.data;
  render();
  if (!managedHere()) { await loadWorkloads(); if (currentRoute() === "overview" && !S.renaming) render(); }
  if (S.overview?.initialized) {
    await Promise.all([loadHealth(), loadBalances(), loadEarnings(), loadOffers()]);
    if (["overview", "earnings"].includes(currentRoute()) && !S.renaming) render(); else { renderMeta(); renderNav(); patchSetupProgress(); patchWorkerView(); }
  }
  if (!S.overview?.initialized && currentRoute() === "keys") loadBalances();
  if (S.overview?.initialized && currentRoute() === "worker") loadWorkerAuto();
  run("cli-update-check", {}, { quiet: true }).then((j) => { S.cliUpdate = j; renderMeta(); });
  let tick = 0;
  setInterval(async () => {
    if (document.visibilityState !== "visible" || !S.token || $("#modal").hidden === false || S.refreshing) return;
    S.refreshing = true;
    try {
    tick++;
    await loadOverview();
    if (S.overview?.initialized) {
      await loadHealth();
      if (tick % 4 === 0) await Promise.all([loadBalances(), loadEarnings()]);
      if (tick % 2 === 0 && !S.batchBusy) await loadOffers();
      if (tick % 2 === 0 && currentRoute() === "worker") await loadWorkerAuto();
    }
    // Only redraw pages without typing in progress.
    // Forms keep their DOM and drafts during background polling, even if a
    // field lost focus. Explicit actions/navigation are the only form redraws.
    if (["overview", "earnings"].includes(currentRoute()) && !S.renaming) render(); else { renderMeta(); renderNav(); patchSetupProgress(); patchWorkerView(); }
    } finally { S.refreshing = false; }
  }, 30000);
}
window.addEventListener("hashchange", () => {
  if (location.hash.includes("pair=")) return;
  S.navigationEpoch++; clearBatchReport();
  S.openStep = null; S.renaming = false; render(); window.scrollTo(0, 0); $("#main").focus({ preventScroll: true });
  if (currentRoute() === "keys") loadBalances();
  if (currentRoute() === "worker") loadWorkerAuto();
});
boot();

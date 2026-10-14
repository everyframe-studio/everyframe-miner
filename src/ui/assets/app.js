"use strict";
// everycli ui — local setup guide and status dashboard.
// Everything shown comes from the same commands the terminal runs. Data is always
// inserted as text (never as HTML), and no request leaves 127.0.0.1.

const S = {
  token: null, session: null, overview: null, wallets: null, help: null,
  health: null, healthAt: 0, offers: null, offersError: null, balances: null, earnings: null,
  cliUpdate: null, workerUpdate: null, hotkeySel: null, registration: null,
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
  try { res = await fetch(path, opts); } catch {
    return { ok: false, status: 0, data: { error: { code: "ui_offline", message: "The UI server is not reachable. Is everycli ui still running?" } } };
  }
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
  "apply-api-keys": "Apply keys to worker", "resume": "Start accepting work", "stop": "Stop worker",
  "worker-update": "Update worker", "reconcile": "Re-sync operation", "remove-api-key": "Remove key",
  "balances-publish": "Publish balances",
  "label": "Save worker label", "logs": "Worker logs",
};

// Runs a CLI command. Questions come back to the browser; approving one re-runs
// the command with that exact question allowed.
async function run(action, params = {}, opts = {}) {
  const approvals = [];
  const act = opts.quiet ? null : Activity.add(opts.label || LABELS[action] || action);
  for (;;) {
    const r = await api("/api/run", { action, params, approvals });
    if (!r.ok) {
      const job = { state: "failed", result: { error: r.data.error || { message: "Request failed." } } };
      act?.finish(job); return job;
    }
    const job = await poll(r.data.job, act);
    if (job.state === "confirm") {
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
  for (;;) {
    const r = await api("/api/jobs/" + id);
    if (!r.ok) return { state: "failed", result: { error: r.data.error || { message: "Lost track of this operation." } } };
    act?.log(r.data.log || []);
    if (r.data.state !== "running") return r.data;
    await new Promise((ok) => setTimeout(ok, 700));
  }
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
    return {
      log(lines) { log.textContent = lines.join("\n"); },
      cancel() { setIcon("warn", "STOPPED"); msg.textContent = "Cancelled. Nothing was changed by this step."; setTimeout(() => box.remove(), 6000); },
      finish(job) {
        const err = errorOf(job), v = value(job);
        if (job.state === "done") {
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
  },
};
function errorView(err) {
  if (!err) return h("span", null, "Something went wrong.");
  return h("div", null,
    h("div", { class: "err-msg" }, err.message || err.code || "Something went wrong."),
    err.next ? h("div", { class: "small muted" }, err.next) : null,
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
    b.disabled = true;
    try { await onclick(b); } finally { if (b.isConnected) b.disabled = false; }
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

const PROVIDERS = {
  phala: ["Phala Cloud", "Hosting. Your worker runs in a sealed (TEE) machine billed to your Phala account."],
  fal: ["fal", "Most video, image and audio models on the network (for example H3 Max, Seedance 2.5, Flux 3)."],
  vivix: ["Vivix", "Vivix W1 world video. Needs Vivix W credits (a W credit pack); A credits do not work."],
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
async function loadBalances() { if (S.overview?.initialized) S.balances = await run("balances", {}, { quiet: true }); }
async function loadEarnings() { if (S.overview?.initialized) S.earnings = await run("earnings", {}, { quiet: true }); }
async function loadOffers(quiet = true) {
  const job = await run("offers", {}, { quiet });
  S.offers = value(job); S.offersError = errorOf(job);
}

// ---------- shell ----------
const ROUTES = [["overview", "Overview"], ["setup", "Setup guide"], ["models", "Models"], ["keys", "API keys"], ["worker", "Worker"], ["help", "Help"]];
function route() { const m = location.hash.match(/^#\/(\w+)/); return m && ROUTES.some(([r]) => r === m[1]) ? m[1] : null; }
function currentRoute() { return route() || (S.overview?.initialized ? "overview" : "setup"); }
function go(r) { location.hash = "#/" + r; }

function renderMeta() {
  const s = S.session, o = S.overview;
  if (!s) return;
  const [label, tone] = phaseLabel(o?.deployment?.phase);
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
    label, r === "setup" && steps.length ? h("span", { class: "count" }, done + "/" + steps.length) : null)));
}
function render() {
  if (!S.token) return renderUnpaired();
  renderMeta(); renderNav();
  const r = currentRoute();
  const page = { overview: pageOverview, setup: pageSetup, models: pageModels, keys: pageKeys, worker: pageWorker, help: pageHelp }[r];
  $("#main").replaceChildren(page());
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
    if (job.state === "failed") { status.replaceChildren(errorView(errorOf(job))); return; }
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
function pageOverview() {
  const o = S.overview;
  const root = h("div", null, h("h1", null, "Overview"),
    h("p", { class: "lede" }, "Live status of your miner. Refreshes every 30 seconds while this tab is open."));
  if (!o) return root;
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
  const failing = checks.filter((c) => checkState(c) === "FAIL").length, warning = checks.filter((c) => checkState(c) === "WARN").length;
  const [phase, tone] = phaseLabel(o.deployment.phase);
  const accepting = findCheck("Accepting new work"), attest = findCheck("Workload attestation");
  const activity = health?.activity || {};
  const activeOffers = checks.filter((c) => (c.name || "").startsWith("Model: ")).length;
  root.append(h("div", { class: "grid" },
    workerStat(phase, tone),
    stat("Readiness", !S.health ? "Checking…" : (health?.ok ? "All checks pass" : (failing + warning) + " need attention"),
      S.healthAt ? "Checked " + ago(S.healthAt) : "", !S.health ? "" : health?.ok ? "ok" : failing ? "bad" : "warn"),
    stat("Accepting work", accepting ? (accepting.ok ? "Yes" : "No") : "—", accepting ? "" : "Unknown until checked", accepting ? (accepting.ok ? "ok" : "warn") : ""),
    stat("Attestation", attest ? (attest.ok ? "Verified" : "Not yet") : "—", "", attest ? (attest.ok ? "ok" : "warn") : ""),
    stat("Active offers", String(activeOffers), activeOffers ? "" : "Choose models to serve", activeOffers ? "ok" : "warn"),
    stat("Jobs", (activity.activeJobs ?? 0) + " active", (activity.acceptedJobs ?? 0) + " accepted so far")));
  if (S.workloads?.length) root.append(notice("warn", "A worker for this miner runs in your Phala account, but this computer doesn't manage it",
    h("div", null, "It was probably deployed from another computer. Keep managing it there, or delete or replace it from the ", h("a", { href: "#/worker" }, "Worker page"), ".")));
  if (o.deployment.needsReconcile) root.append(notice("warn", "An operation was interrupted", "Your last change didn't finish cleanly. Re-sync it from the Worker page before anything else; never deploy a second worker."));
  root.append(card("Health checks", [button("Refresh", async () => { await loadHealth(false); render(); })],
    S.health ? (errorOf(S.health) ? errorView(errorOf(S.health)) : healthList(checks)) : h("p", { class: "muted" }, "Checking…"),
    h("p", { class: "small muted" }, "All green means the current readiness checks passed. It does not guarantee traffic, provider funds or earnings.")));
  const jobs = Array.isArray(activity.jobs) ? activity.jobs : [];
  root.append(card("Job activity", null, jobs.length
    ? h("div", { class: "table-wrap" }, h("table", null, h("thead", null, h("tr", null, h("th", null, "State"), h("th", null, "Jobs"))),
      h("tbody", null, jobs.map((j) => h("tr", null, h("td", null, j.state || "—"), h("td", null, String(j.count ?? "—")))))))
    : h("p", { class: "empty" }, "No jobs yet. Zero active jobs is normal while idle."),
    activity.note ? h("p", { class: "small muted" }, activity.note) : null));
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
  return card("Provider balances", [button("Refresh", async () => { S.balances = await run("balances"); render(); })],
    !S.balances ? h("p", { class: "muted" }, "Loading…") : errorOf(S.balances) ? errorView(errorOf(S.balances)) : rows.length
      ? h("div", { class: "table-wrap" }, h("table", null, h("thead", null, h("tr", null, h("th", null, "Provider"), h("th", null, "Remaining"), h("th", null, "Status"), h("th", null, "Checked"))),
        h("tbody", null, rows.map((r) => h("tr", null, h("td", null, providerName(r.provider)),
          h("td", null, balanceAmount(r)),
          h("td", null, h("span", { class: "badge " + (r.status === "ok" && !r.stale ? "ok" : "warn") }, r.status || "—"), r.stale ? h("span", { class: "small muted" }, " stale") : null,
            r.diagnostic ? errorView(r.diagnostic) : null),
          h("td", null, r.checkedAt ? ago(r.checkedAt) : "—"))))))
      : h("p", { class: "empty" }, "No balances available. Add billing keys on the API keys page to see fal or OpenRouter balances."),
    v?.note ? h("p", { class: "small muted" }, v.note) : null);
}
function earningsCard() {
  const v = value(S.earnings);
  const rows = v ? Object.entries(v).filter(([k]) => !["minerId", "asOf"].includes(k)) : [];
  return card("Earnings", null,
    !S.earnings ? h("p", { class: "muted" }, "Loading…") : errorOf(S.earnings) ? errorView(errorOf(S.earnings)) : rows.length
      ? h("dl", { class: "kv" }, rows.map(([k, val]) => [h("dt", null, k.replace(/([A-Z])/g, " $1").toLowerCase()),
        h("dd", null, typeof val === "object" ? JSON.stringify(val) : String(val))]))
      : h("p", { class: "empty" }, "No earnings reported yet."),
    v?.asOf ? h("p", { class: "small muted" }, "As of " + fmtTime(v.asOf) + ". Rewards are alpha emissions for accepted, verified work; nothing is guaranteed.") : null);
}

// ---------- Setup guide ----------
const STEP_INFO = {
  hotkey: "Check your hotkey", phala: "Add your Phala hosting token", providers: "Add a provider API key",
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
    h("p", { class: "small muted" }, done + " of " + steps.length + " steps done"));
  const list = h("ol", { class: "steps" });
  steps.forEach((st, i) => {
    const isDone = stepDone(st), isOpen = open === st.id;
    const body = isOpen ? h("div", { class: "body" }, stepBody(st.id)) : null;
    list.append(h("li", { class: "step " + (isDone ? "done" : "") + (st.id === firstOpen ? " current" : "") },
      h("div", { class: "num" }, isDone ? "✓" : String(i + 1)),
      h("div", { class: "box" },
        h("div", { class: "title", role: "button", tabindex: "0", onclick: () => { S.openStep = isOpen ? "__none" : st.id; render(); },
          onkeydown: (e) => { if (e.key === "Enter") { S.openStep = isOpen ? "__none" : st.id; render(); } } },
          h("h3", null, STEP_INFO[st.id] || st.id),
          h("span", { class: "badge " + (isDone ? "ok" : st.id === firstOpen ? "info" : "") }, isDone ? "DONE" : st.id === firstOpen ? "NEXT" : "")),
        body)));
  });
  root.append(list);
  if (steps.length && done === steps.length) root.append(notice("ok", "Setup complete", "Your worker accepts work. Keep an eye on the Overview page; jobs depend on customer demand and your offers."));
  return root;
}
async function refreshAll() {
  await loadOverview();
  if (!managedHere() && !S.removal) await loadWorkloads();
  if (S.overview?.initialized) await loadHealth();
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
      ["fal", "vivix", "openrouter"].map((p) => keyRow(p, "providers")),
      h("details", { open: ["minimax", "bfl", "replicate", "google", "runway", "luma", "elevenlabs"].some((p) => S.keyStatus[p]) },
        h("summary", null, "Other providers"), ["minimax", "bfl", "replicate", "google", "runway", "luma", "elevenlabs"].map((p) => keyRow(p, "providers"))),
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
  const file = h("input", { type: "text", placeholder: "/path/to/hotkey" });
  const ss58 = h("input", { type: "text", placeholder: "5F… public address" });
  let mode = options.length ? "wallet" : "file";
  const panes = { wallet: field("Wallet and hotkey on this computer", sel), file: field("Hotkey file", file, "An unencrypted sr25519 hotkey file, as created by btcli."), address: field("Public hotkey address", ss58, "Checks registration only. Creating the profile later needs the hotkey file on this computer.") };
  const holder = h("div", null, panes[mode]);
  const seg = h("div", { class: "seg" }, ["wallet", "file", "address"].map((m) => {
    const b = h("button", { class: m === mode ? "on" : "" }, { wallet: "My wallets", file: "Hotkey file", address: "Address only" }[m]);
    b.addEventListener("click", () => { mode = m; [...seg.children].forEach((c) => c.classList.toggle("on", c === b)); holder.replaceChildren(panes[m]); });
    return b;
  }));
  const result = h("div");
  if (S.registration) result.append(registrationView(S.registration));
  return h("div", { class: "stack" },
    h("p", null, "Your hotkey must be registered on SN" + (S.session?.netuid ?? 117) + " before you can mine. This check only reads the chain. everycli never registers, signs a transaction or pays a fee."),
    o?.registration ? notice("ok", "Registered hotkey", o.registration.hotkey) : null,
    seg, holder,
    h("div", { class: "row" }, button("Check registration", async () => {
      let params;
      if (mode === "wallet") { const x = options[Number(sel.value)]; if (!x) return; params = { wallet: x.wallet, hotkey: x.hotkey }; }
      else if (mode === "file") params = { hotkeyFile: file.value };
      else params = { hotkeySs58: ss58.value };
      const job = await run("register-hotkey", params);
      S.registration = job;
      if (value(job)?.registered === true && mode !== "address") S.hotkeySel = params;
      await loadOverview(); render();
    }, "primary")),
    result);
}
function registrationView(job) {
  const v = value(job);
  if (errorOf(job)) return notice("bad", "Check failed", errorView(errorOf(job)));
  if (!v) return null;
  if (v.registered === true) return notice("ok", "Registered on SN" + (v.netuid ?? ""), (v.hotkey || "") + (v.uid != null ? " · UID " + v.uid : ""));
  return notice("warn", "Not registered yet", h("div", null,
    h("p", null, "Register this hotkey with the wallet that owns it (this costs TAO), then check again:"),
    v.registrationCommand ? copyBox(v.registrationCommand) : null));
}
function stepDeploy(o) {
  const limit = h("input", { type: "number", min: "0.01", step: "0.01", value: "0.06", class: "narrow", "aria-label": "Maximum compute USD per hour" });
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
  if (ownLimit) limit = h("input", { type: "number", min: "0.01", step: "0.01", value: "0.06", class: "narrow", "aria-label": "Maximum compute USD per hour for the new worker" });
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
  if (job.state === "cancelled" || job.state === "failed") return;
  S.removal = { appId, redeploy: redeployLimit, started: Date.now(), note: "Waiting for Phala to remove the VM…" };
  render();
  let gone = false;
  for (let i = 0; i < 60 && !gone; i++) {
    await new Promise((ok) => setTimeout(ok, 5000));
    if (recorded) {
      const check = await run("delete", { appId }, { quiet: true });
      gone = value(check)?.phase === "deleted";
    } else {
      await loadWorkloads();
      gone = !(S.workloads || []).some((w) => w.appId === appId && w.status !== "deleted");
    }
    if (S.removal) { S.removal.note = "Waiting for Phala to remove the VM… (" + Math.round((Date.now() - S.removal.started) / 1000) + " s)"; renderRemoval(); }
  }
  const redeploy = S.removal?.redeploy;
  S.removal = null;
  await loadOverview();
  if (!gone) {
    Activity.add("Delete worker").finish({ state: "attention", result: { value: { message: "Phala accepted the deletion but still lists the VM. Check again in a few minutes from the Worker page." } } });
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
  if (!check || check.result === "valid") return { kind: "ok", text: (saving ? "Saved. " : "") + name + " accepted this key." };
  if (check.result === "rejected") return { kind: "bad", text: name + " rejected the saved key. Replace it with a working key." };
  return { kind: "warn", text: (saving ? "Saved, but not verified: " : "Not verified: ") + (CHECK_REASONS[check.reason] || check.reason) + ". It will be used as entered." };
}
function keyForm(provider, after, stayOn) {
  const saved = (S.overview?.keys || []).find((k) => k.provider === provider)?.saved;
  const input = h("input", { type: "password", autocomplete: "off", spellcheck: "false", placeholder: saved ? "Saved · paste a new key to replace" : "Paste key", "aria-label": providerName(provider) + " key" });
  const finish = async () => {
    if (stayOn) S.openStep = stayOn;
    await loadOverview();
    if (after) await after(); else render();
  };
  const save = button(saved ? "Replace" : "Save", async () => {
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
  return h("div", { class: "keyform" },
    h("div", { class: "row" }, h("span", { class: "badge " + (saved ? "ok" : "") }, saved ? "SAVED" : "NOT SAVED"), input, save, test),
    status);
}
function keyRow(provider, stayOn) {
  const [name, hint] = PROVIDERS[provider] || [provider, ""];
  return h("div", { class: "card flat" }, h("h3", null, name), h("p", { class: "small muted" }, hint), keyForm(provider, null, stayOn));
}
function pageKeys() {
  const o = S.overview, keys = o?.keys || [];
  const deployed = !!o?.deployment?.appId;
  const groups = [["hosting", "Hosting"], ["generation", "Generation providers"], ["billing", "Balance checks (optional)"]];
  const root = h("div", null, h("h1", null, "API keys"),
    h("p", { class: "lede" }, "Keys are stored only on this computer, in an owner-only file, and sealed into your worker. This page never shows a saved key again. To revoke a leaked key, also revoke it at the provider."));
  if (deployed) root.append(notice("", "Changed a generation key?", h("div", null,
    h("p", null, "Your running worker keeps its current keys until you apply new ones. Applying pauses new work, restarts the worker with the new keys and waits for fresh attestation."),
    button("Apply keys to worker", async () => { await run("apply-api-keys"); await refreshAll(); }))));
  for (const [kind, title] of groups) {
    const rows = keys.filter((k) => k.kind === kind);
    root.append(card(title, null, h("ul", { class: "keylist" }, rows.map((k) => h("li", null,
      h("div", null, h("b", null, providerName(k.provider)), h("div", { class: "small muted" }, (PROVIDERS[k.provider] || [, ""])[1])),
      h("div", { class: "row" }, keyForm(k.provider),
        k.saved ? button("Remove", async () => { await run("remove-api-key", { provider: k.provider }); await loadOverview(); render(); }, "small danger") : null))))));
  }
  return root;
}

// ---------- Models ----------
function readiness(r) {
  const b = r.blockers || [];
  if (b.includes("integration_required")) return ["Integration pending", "warn"];
  if (b.includes("pricing_required")) return ["Pricing required", "warn"];
  if (b.includes("worker_release_required")) return ["Worker update required", "warn"];
  if (b.includes("worker_offline")) return ["Worker offline", "bad"];
  if (b.includes("worker_capabilities_unknown")) return ["Worker check required", "warn"];
  if (b.includes("worker_provider_required")) return ["Add provider key to worker", "warn"];
  if (r.offerable === true) return ["Available", "ok"];
  return ["Not reported", ""];
}
function pageModels() {
  const root = h("div", null, h("h1", null, "Models"),
    h("p", { class: "lede" }, "Models the network can route to you, and your offer for each. The discount is taken off the base miner reward, not your provider's price; your provider still bills its normal rate."));
  const refresh = button("Refresh", async () => { await loadOffers(false); render(); });
  if (!S.offers && !S.offersError) { root.append(card("Offers", [refresh], h("p", { class: "muted" }, "Loading…"))); loadOffers().then(render); return root; }
  if (S.offersError) { root.append(card("Offers", [refresh], errorView(S.offersError))); return root; }
  let rows = S.offers.offers || [];
  const filters = [["ready", "Ready for my keys"], ["active", "My active offers"], ["all", "All models"]];
  const seg = h("div", { class: "seg" }, filters.map(([f, label]) => h("button", { class: S.offerFilter === f ? "on" : "", onclick: () => { S.offerFilter = f; render(); } }, label)));
  if (S.overview?.initialized && S.offerFilter === "active") rows = rows.filter((r) => r.offered);
  if (S.overview?.initialized && S.offerFilter === "ready") rows = rows.filter((r) => r.offered || (readiness(r)[0] === "Available" && r.localKeyConfigured !== false));
  root.append(card("Offers", [seg, refresh], rows.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm" },
    h("thead", null, h("tr", null, h("th", null, "Model"), h("th", null, "Provider"), h("th", null, "Readiness"), h("th", null, "Your offer"), h("th", null, "Base reward"), h("th", null, "Change"))),
    h("tbody", null, rows.map(offerRow)))) : h("p", { class: "empty" }, S.offerFilter === "ready" ? "No models are ready for your saved keys yet. Try \"All models\" to see what each one needs." : "Nothing to show."),
    h("p", { class: "small muted" }, "Base reward is the reference value per job (five-second clip for video; studio jobs are repriced per settings). Offers never guarantee traffic or profit.")));
  return root;
}
function offerRow(r) {
  const [ready, tone] = readiness(r);
  const disc = h("input", { type: "number", min: "0", max: "99.9", step: "0.5", value: r.offered && r.discountBp != null ? String(r.discountBp / 100) : "0", class: "tiny", "aria-label": "Discount percent for " + (r.name || r.model) });
  const baseMicrousd = r.baseMinerRewardMicrousd ?? r.pricing?.minerRewardMicrousd;
  const base = typeof baseMicrousd === "number" ? baseMicrousd / 1e6 : null;
  const yours = r.offered ? (r.pricingCurrent ? "Active · " + ((r.discountBp || 0) / 100).toFixed(2) + "% off" : "Stale pricing") : "Not offered";
  return h("tr", null,
    h("td", { "data-label": "Model" }, h("b", null, r.name || r.model), h("span", { class: "id" }, r.model)),
    h("td", { "data-label": "Provider" }, providerName(r.provider || ""), r.localKeyConfigured === false ? h("div", { class: "small muted" }, "No key saved") : null),
    h("td", { "data-label": "Readiness" }, h("span", { class: "badge " + tone }, ready)),
    h("td", { "data-label": "Your offer" }, h("span", { class: "badge " + (r.offered ? (r.pricingCurrent ? "ok" : "warn") : "") }, yours)),
    h("td", { "data-label": "Base reward" }, base != null ? fmtUsd(base, base < 1 ? 3 : 2) : "—"),
    h("td", { "data-label": "Change" }, !S.overview?.initialized ? h("a", { href: "#/setup" }, "Set up to offer") : h("div", { class: "row nowrap" }, disc, h("span", { class: "small muted" }, "% off"),
      button(r.offered ? "Update" : "Offer", async () => { await run("offer", { model: r.model, discountPct: disc.value }); await loadOffers(); await loadHealth(); render(); }, "small primary"),
      r.offered ? button("Withdraw", async () => { await run("offer", { model: r.model, withdraw: true }); await loadOffers(); await loadHealth(); render(); }, "small") : null)));
}

// ---------- Worker ----------
function pageWorker() {
  const o = S.overview, d = o?.deployment || {}, p = o?.profile || {};
  const [phase, tone] = phaseLabel(d.phase);
  const root = h("div", null, h("h1", null, "Worker"),
    h("p", { class: "lede" }, "Manage the machine that does the work. Every action shows what it will do and asks before anything changes or costs money. Closing this page never stops your worker."));
  if (!o?.initialized) { root.append(notice("warn", "No miner profile yet", "Finish the setup guide first.")); return root; }
  if (S.removal) root.append(removalView());
  else if (S.workloads?.length) root.append(workloadsNotice(null));
  else if (!managedHere()) root.append(notice("", "No worker deployed from this computer", h("div", null, "Deploy one from step 5 of the ", h("a", { href: "#/setup" }, "setup guide"), ".")));
  root.append(h("div", { class: "grid" },
    stat("State", phase, d.appId ? "App " + String(d.appId).slice(0, 10) + "…" : "", tone),
    nameStat(),
    stat("Compute price", d.computeHourlyUsd ? "$" + d.computeHourlyUsd + "/h" : "—", "Storage extra"),
    stat("Configuration", p.configurationExpiresAt ? daysLeft(p.configurationExpiresAt) + " days left" : "—", "Renew with everycli init")));
  if (d.needsReconcile) root.append(notice("warn", "Confirm worker state", h("div", null,
    h("p", null, "A lifecycle operation is pending. Re-sync checks its result at Phala and the coordinator without repeating the cloud operation."),
    button("Re-sync operation", async () => { await run("reconcile"); await refreshAll(); }, "primary"))));
  const ph = d.phase || "not_deployed", deployed = !!d.appId && !["stopped", "stopping"].includes(ph);
  const accepting = findCheck("Accepting new work");
  const when = (ok, el, why) => { if (!ok) { el.disabled = true; el.title = why; } return el; };
  const startLimit = h("input", { type: "number", min: "0.01", step: "0.01", value: "0.06", class: "narrow", "aria-label": "Maximum compute USD per hour" });
  const actions = [
    ["Re-sync state", "Confirm completed starts, shutdowns or interrupted operations on this same VM.",
      when(!!d.appId, button("Re-sync state", async () => { await run("reconcile"); await refreshAll(); }), "No deployment recorded.")],
    ["Start accepting work", "Lets the network route paid jobs to this worker again.",
      when(["awaiting_readmission", "drained"].includes(ph) || (ph === "running" && accepting && !accepting.ok),
        button("Resume", async () => { await run("resume"); await refreshAll(); }, "primary"), "Already accepting work, or the worker isn't ready.")],
    ["Pause new work", "Finishes current jobs, then stops taking new ones. The machine keeps running and billing.",
      when(ph === "running", button("Pause", async () => { await run("stop", { drainOnly: true }); await refreshAll(); }), "Only a running worker can be paused.")],
    ["Update worker", "Checks for a newer signed worker release. Updating pauses work, updates the same machine, waits for attestation and restores the previous state.",
      h("div", { class: "row" }, when(managedHere(), button("Check", async () => { S.workerUpdate = await run("worker-update-check"); render(); }), "No worker deployed from this computer."),
        when(managedHere(), button("Update", async () => { await run("worker-update"); await refreshAll(); }, "primary"), "No worker deployed from this computer."))],
    ["Apply saved keys", "Pushes keys you changed on the API keys page to this worker (pauses, restarts, re-attests).",
      when(deployed, button("Apply keys", async () => { await run("apply-api-keys"); await refreshAll(); }), "No running worker.")],
    ["Shut down", "Drains work, then shuts the machine down. Storage may still be billed; nothing is deleted.",
      when(deployed, button("Shut down", async () => { await run("stop"); await refreshAll(); }, "danger"), "No running worker.")],
    ["Emergency shutdown", "Use if the coordinator is unavailable. Can interrupt paid jobs; storage remains billable. The CLI still verifies the exact VM.",
      when(!!d.appId && ph !== "deleted", button("Emergency shutdown", async () => { await run("stop", { force: true }); await refreshAll(); }, "danger"), "No deployment recorded.")],
    ["Delete worker", "Permanently deletes this VM and its storage. Billing for it stops; jobs in progress are lost. You can deploy a new worker afterwards.",
      when(!!d.appId && !["deleted"].includes(ph), button(["deleting", "delete_intent"].includes(ph) ? "Check deletion" : "Delete", async () => { await removeWorker(d.appId, true, null); }, "danger"), "No worker recorded on this computer.")],
    ["Redeploy", "Deletes this worker, waits until Phala removes it, then deploys a fresh one at up to the price below.",
      when(!!d.appId && !["deleted", "deleting", "delete_intent"].includes(ph), button("Delete and deploy again", async () => { await removeWorker(d.appId, true, startLimit.value); }, "danger"), "No worker recorded on this computer.")],
    ["Start a stopped worker", "Starts the same machine again at up to this compute price.", h("div", { class: "row" }, startLimit, h("span", { class: "small muted" }, "USD/h max"),
      when(ph === "stopped", button("Start", async () => { await run("start", { maxHourlyUsd: startLimit.value }); await refreshAll(); }), "Only a stopped worker can be started."))],
  ];
  root.append(card("Actions", null, h("ul", { class: "checks" }, actions.map(([t, desc, ctl]) => h("li", { class: "action-row" },
    h("span"), h("div", { class: "row" }, h("div", { class: "grow" }, h("div", { class: "name" }, t), h("div", { class: "detail" }, desc)), ctl))))));
  root.append(card("Worker logs", [button("Read logs", async () => { await run("logs"); })],
    h("p", { class: "small muted" }, "Read bounded, sanitized worker events. Private logs may require the Phala console.")));
  if (S.workerUpdate) root.append(card("Worker update check", null, value(S.workerUpdate) ? h("pre", { class: "out" }, S.workerUpdate.result.text) : errorView(errorOf(S.workerUpdate))));
  root.append(card("Profile", null, h("dl", { class: "kv" },
    h("dt", null, "Miner"), h("dd", { class: "mono" }, p.minerId || "—"),
    h("dt", null, "Hotkey"), h("dd", { class: "mono" }, p.hotkey || "—"),
    h("dt", null, "Network"), h("dd", null, (S.session?.network || "") + " · SN" + (S.session?.netuid || "")),
    h("dt", null, "Signed worker name"), h("dd", { class: "mono wrap" }, p.workerName || "—"),
    h("dt", null, "Profile folder"), h("dd", { class: "mono wrap" }, S.session?.stateDir || "—"))));
  root.append(card("everycli", [button("Check for update", async () => { S.cliUpdate = await run("cli-update-check"); render(); })],
    h("p", null, "Installed: everycli " + (S.session?.version || "")),
    S.cliUpdate ? (value(S.cliUpdate) ? h("pre", { class: "out" }, S.cliUpdate.result.text) : errorView(errorOf(S.cliUpdate))) : null,
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
  await loadOverview();
  const w = await api("/api/wallets"); if (w.ok) S.wallets = w.data;
  render();
  if (!managedHere()) { await loadWorkloads(); render(); }
  if (S.overview?.initialized) {
    await Promise.all([loadHealth(), loadBalances(), loadEarnings()]);
    if (currentRoute() === "overview" && !S.renaming) render(); else { renderMeta(); renderNav(); }
  }
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
    }
    // Only redraw pages without typing in progress.
    // Forms keep their DOM and drafts during background polling, even if a
    // field lost focus. Explicit actions/navigation are the only form redraws.
    if (currentRoute() === "overview" && !S.renaming) render(); else { renderMeta(); renderNav(); }
    } finally { S.refreshing = false; }
  }, 30000);
}
window.addEventListener("hashchange", () => {
  if (location.hash.includes("pair=")) return;
  S.openStep = null; S.renaming = false; render(); window.scrollTo(0, 0); $("#main").focus({ preventScroll: true });
});
boot();

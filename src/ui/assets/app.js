"use strict";
// everycli ui — local setup guide and status dashboard.
// Everything shown comes from the same commands the terminal runs. Data is always
// inserted as text (never as HTML), and no request leaves 127.0.0.1.
// Only non-secret model notification acknowledgments are kept in localStorage.

const S = {
  autoOffers: null, autoTried: false, autoError: null, autoBusy: false, autoRequest: 0, autoDrafts: new Map(), autoExpanded: new Set(),
  termsDraft: null,
  token: null, session: null, overview: null, wallets: null, help: null,
  health: null, healthAt: 0, offers: null, offersError: null, balances: null, earnings: null,
  jobDetail: null, jobDetailError: false, jobDetailBusy: false, jobDetailRequest: 0, history: null, historyBusy: false, historyRequest: 0, historyStatus: "all", historyModel: "", historyModelDraft: "", historyCursors: [null], historyPage: 0, openAttempt: null, earningsBusy: false,
  cliUpdate: null, workerUpdate: null, workerUpdateBusy: false, workerUpdateRunning: false, workerUpdateAt: 0, resumeResult: null, hotkeySel: null, registration: null,
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
const ICONS = {
  overview: "M4 13h6V4H4zM14 20h6v-9h-6zM14 4v4h6V4zM4 20h6v-3H4z",
  setup: "M9 11l2 2 4-4M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z",
  models: "M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12L4 7.5",
  jobs: "M4 6h16M4 12h16M4 18h10",
  earnings: "M4 19V9M10 19V5M16 19v-7M22 19H2",
  keys: "M14 10a4 4 0 1 0-3.2 3.9L13 16h2v2h2v2h3v-3l-5.1-5.1A4 4 0 0 0 14 10zM9.5 9.5h.01",
  worker: "M4 5h16v6H4zM4 13h16v6H4zM8 8h.01M8 16h.01",
  help: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17h.01",
  pulse: "M3 12h4l3-8 4 16 3-8h4",
  offers: "M20 12l-8 8-9-9V3h8zM7.5 7.5h.01",
  bolt: "M13 2L4 14h7l-1 8 9-12h-7z",
  copy: "M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1",
  check: "M5 12.5l4.5 4.5L19 7",
};
function icon(name, cls) {
  const ns = "http://www.w3.org/2000/svg", svg = document.createElementNS(ns, "svg"), path = document.createElementNS(ns, "path");
  svg.setAttribute("viewBox", "0 0 24 24"); svg.setAttribute("aria-hidden", "true"); svg.setAttribute("class", "icon " + (cls || ""));
  path.setAttribute("d", ICONS[name] || ""); svg.append(path); return svg;
}
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
function meter(frac) {
  const fill = h("span");
  fill.style.width = Math.max(2, Math.round(Math.max(0, Math.min(1, frac)) * 100)) + "%";
  return h("span", { class: "meter", "aria-hidden": "true" }, fill);
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
  "terms-accept": "Accept Miner Terms",
  "worker-update-check": "Check for worker update", "cli-update-check": "Check for CLI update", "init": "Create miner profile",
  "offer": "Update model offer", "deploy": "Deploy worker", "start": "Start worker", "activate": "Activate keys",
  "auto-offers-status": "Automatic offer status", "auto-offers-preview": "Provider price preview", "auto-offers-enable": "Enable automatic offers", "auto-offers-disable": "Disable automatic offers",
  "offer-batch": "Update model offers", "jobs": "Job history",
  "apply-api-keys": "Apply keys to worker", "resume": "Start accepting work", "stop": "Shut down worker",
  "worker-update": "Update worker", "reconcile": "Re-sync operation", "remove-api-key": "Remove key",
  "worker-update-abort": "Cancel worker update", "worker-auto-enable": "Turn on automatic worker updates",
  "worker-auto-disable": "Turn off automatic worker updates", "worker-auto-status": "Automatic update status",
  "balances-publish": "Publish balances",
  "label": "Save worker label", "logs": "Worker logs",
  "reconnect": "Reconnect miner profile",
};

// Runs a CLI command. Questions come back to the browser; approving one re-runs
// the command with that exact question allowed.
async function run(action, params = {}, opts = {}) {
  const changes = new Set(["auto-offers-enable", "auto-offers-disable", "terms-accept", "register-hotkey", "init", "reconnect", "reconcile", "offer", "offer-batch", "deploy", "start", "activate", "apply-api-keys", "resume", "stop", "label", "delete", "worker-update", "worker-update-abort", "worker-auto-enable", "worker-auto-disable", "remove-api-key", "balances-publish"]);
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
    const job = await poll(jobId, act, opts.onLog);
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
async function poll(id, act, onLog) {
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
    onLog?.(r.data.log || []);
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
    Array.isArray(err.automaticOfferIssues) ? h("ul", { class: "small" }, err.automaticOfferIssues.map(r =>
      h("li", null, r.model + ": " + automaticReason(r.reason) + " (" + r.reason + ")"))) : null,
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
function stat(label, val, sub, tone, glyph) { return h("div", { class: "stat " + (tone || "") }, glyph ? icon(glyph, "stat-icon") : null, h("div", { class: "label" }, label), h("div", { class: "value" }, val), sub ? h("div", { class: "sub" }, sub) : null); }
function card(title, actions, ...body) {
  return h("section", { class: "card" }, h("div", { class: "card-head" }, h("h2", null, title), actions ? h("div", { class: "row" }, actions) : null), ...body);
}
function checkState(c) { return c.state || (c.ok ? "PASS" : "FAIL"); }
function checksList(checks, withFix = false) {
  return h("ul", { class: "checks" }, (checks || []).map((c) => {
    const fix = withFix ? checkFix(c) : null;
    return h("li", null,
      h("span", { class: "badge " + checkState(c) }, checkState(c)),
      h("div", null, h("div", { class: "name" }, c.name), h("div", { class: "detail" }, c.message || c.detail || ""),
        fix ? h("div", { class: "fix" }, h("p", null, h("b", null, "How to fix: "), fix[0]), h("div", { class: "row" }, fix.slice(1))) : null));
  }));
}
function healthList(checks) {
  const problems = checks.filter((c) => checkState(c) !== "PASS"), passed = checks.filter((c) => checkState(c) === "PASS");
  return h("div", null,
    problems.length ? checksList(problems, true) : notice("ok", "All checks pass", null),
    passed.length ? h("details", null, h("summary", null, passed.length + " checks passed"), checksList(passed)) : null);
}
// ---------- how to fix a readiness problem ----------
// Opens the page that fixes a check and briefly highlights the control to use there.
async function goFix(hash, selector, prep) {
  if (location.hash !== hash) { history.pushState(null, "", hash); S.navigationEpoch++; clearBatchReport(); S.renaming = false; S.openStep = null; }
  prep?.(); render(); window.scrollTo(0, 0);
  const flash = () => {
    const el = selector && document.querySelector(selector);
    if (!el) return null;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    el.classList.remove("spotlight"); void el.offsetWidth; el.classList.add("spotlight");
    setTimeout(() => el.classList.remove("spotlight"), 2600);
    return el;
  };
  const el = flash();
  // These pages refresh some cards after opening; highlight again if ours was replaced.
  if (currentRoute() === "keys") await loadBalances();
  if (currentRoute() === "worker") await loadWorkerTools();
  if (selector && !el?.isConnected) flash();
}
async function recheckHealth() {
  // Re-run only the health rows; balances and offers keep their last result.
  const keep = (S.boot?.steps || []).filter(st => !st.id.startsWith("health-") && st.id !== "readiness");
  Boot.begin("Re-checking health"); const checking = healthStep(); S.boot.steps.push(...keep); Boot.render();
  await checking; Boot.finish(); render();
}
// Returns [what to do, ...buttons or links] for a failing or warning check, or null.
function checkFix(c) {
  const name = c.name || "", detail = String(c.message || c.detail || "");
  const go = (label, hash, selector, prep) => h("a", { class: "btn small", href: hash, onclick: e => { e.preventDefault(); goFix(hash, selector, prep); } }, label);
  const again = () => button("Check again", recheckHealth, "small");
  const resync = () => button("Re-sync worker", async () => { await run("reconcile"); await refreshAll(); }, "small");
  const worker = (label, selector) => go(label, "#/worker", selector);
  const setupDeploy = (label) => go(label, "#/setup", '[data-step="deploy"]', () => { S.openStep = "deploy"; });
  const models = (label, filter, model) => go(label, modelsHref(filter), model ? 'tr[data-model="' + CSS.escape(model) + '"]' : "#offers-table",
    () => { S.offerFilter = filter; S.modelProvider = ""; S.modelKind = ""; S.modelQuery = model || ""; S.selectedOffers.clear(); });
  if (name.startsWith("Model: ")) {
    const model = name.slice(7);
    if (/stale|unverified/i.test(detail)) return ["Renew this offer at the current price on the Models page.", models("Review offer", "active", model)];
    return ["Check this model on the Models page. If your worker can't serve it, withdraw the offer so no jobs are routed to it.",
      models("Open in Models", "active", model),
      button("Withdraw offer", async () => { const job = await run("offer", { model, withdraw: true }); if (job.state === "done") { await loadOffers(); await recheckHealth(); } }, "small")];
  }
  if (name.startsWith("Provider: ")) return ["Your worker's release doesn't allow " + providerName(name.slice(10)) + " yet. Update the worker to the latest signed release.", worker("Update worker", "#worker-software")];
  switch (name) {
    case "Local miner profile": return ["Finish the setup guide to create your miner profile.", go("Open setup guide", "#/setup", "#setup-guide .step.current")];
    case "Deployment configuration": return ["Reconnect your profile to refresh its signed setup. It keeps your keys and worker.", reconnectButton()];
    case "Phala credential": return ["Save your Phala Cloud token on the API keys page.", go("Add Phala token", "#/keys", '[data-provider-key="phala"]')];
    case "Provider credentials": return ["Save at least one generation provider key, such as fal or OpenRouter.", go("Add a provider key", "#/keys", "#keys-generation")];
    case "Registry": return ["This release needs a registry credential. Deploy from the setup guide to supply it.", setupDeploy("Open deploy step")];
    case "Operation journal": return ["An earlier worker operation didn't finish. Re-sync checks its result without repeating it.", resync()];
    case "Coordinator": return ["everycli couldn't reach the EveryFrame coordinator. Check this computer's internet connection, then check again.", again()];
    case "Miner enabled": return ["The coordinator has disabled this miner. Another VM won't change that; contact EveryFrame support.", go("Get help", "#/help", null)];
    case "Workload attestation":
    case "Attestation session": return ["Verification usually finishes a few minutes after the worker starts or gets new keys. Check again shortly; if it doesn't clear, re-sync the worker.", again(), resync()];
    case "Accepting new work":
      if (/draining|paused/i.test(detail)) return ["Your worker is paused. Resume it to accept paid jobs again.", button("Start accepting work", async () => { await startAcceptingWork(); await recheckHealth(); }, "small primary")];
      return ["Routing is off for this miner on the coordinator. Check the worker's state, then check again.", worker("Open Worker", '[data-action="Re-sync state"]'), again()];
    case "Current model bid":
    case "Active model support": return ["Offer at least one model with a current price, so the network can send you jobs.", models("Choose models", "ready")];
    case "Deployment binding": return ["The coordinator's workload doesn't match this computer's record. Re-sync the worker; don't deploy another one.", resync(), worker("Open Worker", '[data-action="Re-sync state"]')];
    case "Phala workload":
      if (/No workload deployed/i.test(detail)) return ["No worker is deployed from this profile yet. Deploy one from the setup guide.", setupDeploy("Deploy a worker")];
      if (/not running/i.test(detail)) return ["Your worker is stopped. Start the same machine again from the Worker page; don't deploy another.", worker("Start worker", '[data-action="Start a stopped worker"]')];
      return ["everycli couldn't confirm the worker in Phala Cloud. Re-sync it, then check again.", resync(), again()];
    case "Worker heartbeat": return ["The worker hasn't checked in within the last 30 seconds. Look at its state and logs on the Worker page; don't deploy another worker.", worker("Open worker logs", "#worker-logs"), again()];
  }
  return null;
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
  alibaba: ["Alibaba Cloud", "Wan 3.0 and Qwen Image 3.0 through Model Studio in Singapore. Use a matching regional DashScope API key."],
  byteplus: ["BytePlus", "Seedance 2.5 and Seedream 5.0 through ModelArk in Singapore. Use your BytePlus API key."],
  volcengine: ["Volcengine", "Seedance 2.5 and Seedream 5.0 through Ark in Beijing. Use your Volcengine Ark API key."],
  "luma-agents": ["Luma Agents", "Native UNI images and Ray video. Uses a Luma Agents API key."],
  openai: ["OpenAI", "Direct GPT Image and audio generation using your OpenAI API key."],
  xai: ["xAI", "Direct Grok image, video and speech generation using your xAI API key."],
  phala: ["Phala Cloud", "Hosting. Your worker runs in a sealed (TEE) machine billed to your Phala account."],
  fal: ["fal", "Most video, image and audio models on the network (for example H3 Max, Seedance 2.5, Flux 3)."],
  vivix: ["Vivix", "Vivix W1 world video. Needs Vivix W credits (a W credit pack); A credits do not work."],
  pika: ["Pika", "Pika H3 Flex video with native audio, keyframes and visual references. Use a Pika developer API key. Your API balance is checked without generating media."],
  kling: ["Kling (direct)", "Kling developer API key, not a Fal key or legacy access/secret pair. API resource packs are separate from the web subscription. Model availability requires a signed worker release and pricing."],
  openrouter: ["OpenRouter", "Seedance and image models served through OpenRouter."],
  wavespeed: ["WaveSpeedAI", "Image, video and audio models through WaveSpeedAI. Use a generation-capable API key from wavespeed.ai/accesskey; balance checks do not generate media."],
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
const DIRECT_PROVIDERS = new Set(["alibaba", "byteplus", "volcengine", "luma-agents", "openai", "google", "xai", "elevenlabs", "minimax", "bfl", "kling", "pika", "runway", "luma", "vivix"]);
const compareProviders = (a, b) => Number(!DIRECT_PROVIDERS.has(a)) - Number(!DIRECT_PROVIDERS.has(b))
  || String(providerName(a) || "").localeCompare(String(providerName(b) || ""));

// ---------- startup checks (shown step by step in the status card while the first load runs) ----------
const Boot = {
  begin(title) {
    clearInterval(S.bootTimer);
    S.boot = { title, steps: [], started: Date.now(), finished: null };
    S.bootTimer = setInterval(() => Boot.tick(), 200);
    Boot.render();
  },
  step(id, label, waiting = false) {
    const st = { id, label, state: waiting ? "wait" : "run", detail: "", started: waiting ? null : Date.now(), ended: null };
    S.boot?.steps.push(st); Boot.render();
    const end = (state, detail) => { st.started ??= Date.now(); st.state = state; st.detail = detail || ""; st.ended ??= Date.now(); Boot.render(); };
    return Object.assign(st, { start: () => { if (st.state === "wait") { st.state = "run"; st.started = Date.now(); Boot.render(); } },
      ok: d => end("ok", d), warn: d => end("warn", d), fail: d => end("bad", d) });
  },
  // `st` may be a step listed earlier as waiting; it starts when the work does.
  async track(st, work, summarize) {
    st.start();
    try { await work(st); const [state, detail] = summarize(); st[state === "bad" ? "fail" : state](detail); }
    catch (e) { st.fail(e?.message || "Could not finish this check."); }
  },
  finish() {
    const b = S.boot; if (!b) return;
    b.finished = Date.now(); clearInterval(S.bootTimer); Boot.render();
  },
  tone(b) { return !b.finished ? "run" : b.steps.some(x => x.state === "bad") ? "bad" : b.steps.some(x => x.state === "warn") ? "warn" : "ok"; },
  tick() { for (const el of document.querySelectorAll("[data-since]")) el.textContent = secs(Date.now() - Number(el.dataset.since)); },
  render() {
    const side = $("#hero-side");
    // Fade the list in once, not on every step change or page redraw.
    if (side) { side.replaceChildren(...heroSide()); requestAnimationFrame(() => { side.dataset.shown = "1"; S.bootShown = true; }); return; }
    // Before the status card exists, the loading line names the step in progress.
    const loading = $("#main > p.pad"), cur = S.boot?.steps.findLast(x => x.state === "run");
    if (loading && cur) loading.textContent = cur.label + "…";
  },
};
const secs = (ms) => (Math.max(0, ms) / 1000).toFixed(1) + " s";
const stateIcon = (state) => h("span", { class: "chk-icon " + state, "aria-hidden": "true" }, state === "ok" ? "✓" : state === "warn" ? "!" : state === "bad" ? "×" : "");
const stepTime = (st) => st.ended ? h("span", { class: "chk-time" }, secs(st.ended - st.started)) : st.started ? h("span", { class: "chk-time", "data-since": st.started }, secs(Date.now() - st.started)) : h("span", { class: "chk-time" });
// Shown as waiting while everycli and the profile load, before the real rows exist.
const BOOT_PLANNED = ["Profile and keys", "Coordinator", "Phala worker", "Readiness", "Provider balances", "Model offers"];
function bootList(b) {
  const tone = Boot.tone(b);
  return h("div", { class: "hero-checks " + tone, role: "status", "aria-live": "polite", "aria-label": b.title },
    h("ul", null, (b.steps.length || b.finished ? b.steps : BOOT_PLANNED.map(label => ({ label, state: "wait" }))).map(st => h("li", {
      class: "chk " + st.state + (st.id === "readiness" && ["warn", "bad"].includes(st.state) ? " chk-link" : ""),
      title: st.label + (st.why || st.detail ? ": " + (st.why || st.detail) : "") + (st.id === "readiness" && ["warn", "bad"].includes(st.state) ? ". Click to see how to fix." : ""),
      onclick: st.id === "readiness" && ["warn", "bad"].includes(st.state) ? () => $("#overview-health")?.scrollIntoView({ behavior: "smooth", block: "start" }) : null },
      stateIcon(st.state), h("span", { class: "chk-label" }, st.short || st.label),
      h("span", { class: "chk-detail" }, st.detail || (st.state === "run" ? "checking…" : st.state === "wait" ? "waiting" : "")), stepTime(st)))));
}
function heroSide() { return S.boot ? [bootList(S.boot)] : []; }
// Health runs its stages in order and logs a line as each one starts; each stage is its own row.
const HEALTH_STAGES = [["local miner profile", "Profile and keys"], ["coordinator", "Coordinator"], ["phala cloud", "Phala worker"]];
function healthStep() {
  const rows = HEALTH_STAGES.map(([, label], i) => Boot.step("health-" + i, label, i > 0));
  const ready = Boot.step("readiness", "Readiness", true);
  let at = 0;
  const reach = (n) => { for (; at < n && at < rows.length - 1; at++) { rows[at].ok(); rows[at + 1].start(); } };
  return (async () => {
    try {
      await loadHealth(true, (lines) => {
        const text = lines.filter(l => typeof l === "string").map(l => l.toLowerCase());
        reach(Math.max(0, ...HEALTH_STAGES.map(([key], i) => text.some(l => l.includes(key)) ? i : 0)));
      });
    } catch {}
    const err = errorOf(S.health), v = value(S.health), checks = v?.checks || [];
    // Order from health: profile checks, "Coordinator" and what it reports, "Phala workload", then readiness.
    const ci = checks.findIndex(c => c.name === "Coordinator"), pi = checks.findIndex(c => c.name === "Phala workload");
    const known = ci >= 0 && pi > ci;
    const groups = known ? [checks.slice(0, ci), checks.slice(ci, pi), [checks[pi]]] : [[], [], []];
    const tail = known ? checks.slice(pi + 1) : checks;
    const verdict = (st, list, okText) => {
      const bad = list.find(c => checkState(c) === "FAIL"), warn = list.find(c => checkState(c) === "WARN");
      const pick = bad || warn, why = pick && (pick.name + ": " + (pick.message || pick.detail || checkState(pick).toLowerCase()));
      bad ? st.fail(why) : warn ? st.warn(why) : st.ok(okText);
    };
    rows.forEach((st, i) => {
      if (err && !checks.length) return st.state === "ok" ? null : st.fail(err.message || "Health check failed");
      verdict(st, groups[i], [list => list.length ? list.length + " checks ok" : "ok", () => "verified", () => v?.cloudStatus || "checked"][i](groups[i]));
    });
    ready.start();
    if (err && !checks.length) return ready.fail("not checked");
    const pass = checks.filter(c => checkState(c) === "PASS").length;
    verdict(ready, tail, pass + " of " + checks.length + " pass");
    // The row stays short; its tooltip and the Health checks card name the check.
    if (ready.state !== "ok") { const n = tail.filter(c => checkState(c) !== "PASS").length; ready.why = ready.detail; ready.detail = pass + " of " + checks.length + " pass · " + n + (ready.state === "bad" ? " failing" : n === 1 ? " warning" : " warnings"); Boot.render(); }
  })();
}
function bootSteps() {
  const health = healthStep();
  const bal = Boot.step("balances", "Provider balances", true), offers = Boot.step("offers", "Model offers", true);
  return Promise.all([
    health,
    Boot.track(bal, loadBalances, () => {
      if (errorOf(S.balances)) return ["bad", errorOf(S.balances).message || "Could not read balances"];
      return balanceSummary(value(S.balances)?.balances || []);
    }),
    loadEarnings(),
    Boot.track(offers, () => loadOffers(), () => {
      if (S.offersError) return ["bad", S.offersError.message || "Could not load offers"];
      const on = (S.offers?.offers || []).filter(r => r.offered).length;
      return [on ? "ok" : "warn", on + " active"];
    }),
  ]);
}

// ---------- data loading ----------
async function loadOverview() {
  const r = await api("/api/overview");
  if (r.ok) S.overview = r.data;
  return S.overview;
}
async function loadHealth(quiet = true, onLog) {
  if (!S.overview?.initialized) return;
  S.health = await run("health", {}, { quiet, onLog });
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
const NAV_GROUPS = [["Operate", ["overview", "models", "jobs", "earnings"]], ["Configure", ["setup", "keys", "worker"]], ["Support", ["help"]]];
const OFFER_FILTERS = [["ready", "Ready for my keys"], ["active", "My active offers"], ["not-offered", "Not offered"], ["all", "All models"], ["candidate", "Unavailable"]];
function route() { const m = location.hash.match(/^#\/(\w+)/); return m && ROUTES.some(([r]) => r === m[1]) ? m[1] : null; }
function currentRoute() { return route() || (S.overview?.initialized ? "overview" : "setup"); }
function go(r) { location.hash = "#/" + r; }
function modelsHref(filter, provider = "") {
  const query = new URLSearchParams({ filter });
  if (provider) query.set("provider", provider);
  return "#/models?" + query;
}
function readModelsRoute() {
  if (route() !== "models") return;
  const query = new URLSearchParams(location.hash.split("?")[1] || "");
  if (OFFER_FILTERS.some(([f]) => f === query.get("filter"))) S.offerFilter = query.get("filter");
  if (query.has("filter")) {
    S.modelProvider = Object.hasOwn(PROVIDERS, query.get("provider")) ? query.get("provider") : "";
    S.modelQuery = ""; S.modelKind = ""; S.selectedOffers.clear();
  }
}
function openModels(filter, provider = "", resetSearch = true) {
  S.offerFilter = filter; S.modelProvider = provider; S.selectedOffers.clear(); clearBatchReport();
  if (resetSearch) { S.modelQuery = ""; S.modelKind = ""; }
  if (filter === "all") acknowledgeModels();
  const href = modelsHref(filter, provider);
  if (location.hash !== href) {
    history.pushState(null, "", href); S.navigationEpoch++;
    S.openStep = null; S.renaming = false;
  }
  render();
}
function modelViewLink(label, filter, provider = "", attrs = {}) {
  return h("a", { ...attrs, href: modelsHref(filter, provider), onclick: e => { e.preventDefault(); openModels(filter, provider); } }, label);
}

function renderMeta() {
  const s = S.session, o = S.overview;
  if (!s) return;
  const [label, tone] = workerState();
  $("#meta").replaceChildren(...[
    h("span", { class: "pill brand-pill" }, (s.network === "testnet" ? "Testnet · SN" : "Mainnet · SN") + s.netuid),
    o?.initialized ? h("span", { class: "pill worker-pill " + tone }, h("span", { class: "dot" }), "Worker: " + label) : null,
    h("span", { class: "pill hide-sm" }, "everycli " + s.version),
    S.cliUpdate && value(S.cliUpdate)?.updateAvailable ? h("span", { class: "pill warn" }, h("span", { class: "dot" }), "CLI update available") : null].filter(Boolean));
}
function renderNav() {
  const current = currentRoute();
  const steps = S.overview?.steps || [];
  const done = steps.filter((x) => stepDone(x)).length;
  const labels = Object.fromEntries(ROUTES);
  $("#nav").replaceChildren(...NAV_GROUPS.map(([group, routes]) => h("div", { class: "nav-group" }, h("div", { class: "nav-title", "aria-hidden": "true" }, group),
    routes.map(r => h("a", { href: "#/" + r, class: r === current ? "active" : "", "aria-current": r === current ? "page" : null },
      icon(r), h("span", { class: "nav-label" }, labels[r]),
      r === "models" && S.newModels.size ? h("span", { class: "count new", "aria-label": S.newModels.size + " new models" }, S.newModels.size) : null,
      r === "setup" && steps.length ? h("span", { class: "count" + (done === steps.length ? " complete" : "") }, done + "/" + steps.length) : null)))));
}
function render() {
  if (!S.token) return renderUnpaired();
  renderMeta(); renderNav();
  const r = currentRoute();
  const page = { overview: pageOverview, setup: pageSetup, models: pageModels, jobs: pageJobs, earnings: pageEarnings, keys: pageKeys, worker: pageWorker, help: pageHelp }[r];
  $("#main").replaceChildren(page());
  if (S.overview?.terms?.accepted === false && r !== "setup") {
    const banner = notice("warn", "Miner Terms acceptance required", h("div", null,
      h("p", null, "Before joining or enabling new work, agree that you obtain provider resale permissions and negotiate provider discounts directly."),
      h("a", { href: "#/setup", onclick: () => { S.openStep = "terms"; } }, "Review and accept Miner Terms")));
    const lede = $("#main .lede");
    if (lede) lede.after(banner); else $("#main").prepend(banner);
  }
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
  return h("div", { class: "stat " + (tone || "") }, icon("worker", "stat-icon"),
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
  const copy = h("button", { class: "copy-icon", type: "button", title: "Copy hotkey", "aria-label": "Copy hotkey" }, icon("copy"));
  copy.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(key);
      copy.replaceChildren(icon("check")); copy.classList.add("copied"); copy.title = "Copied"; copy.setAttribute("aria-label", "Hotkey copied");
      setTimeout(() => { copy.replaceChildren(icon("copy")); copy.classList.remove("copied"); copy.title = "Copy hotkey"; copy.setAttribute("aria-label", "Copy hotkey"); }, 1600);
    } catch { if (!copy.closest(".miner-identity").nextElementSibling?.matches(".copy")) { const box = copyBox(key); box.classList.add("plain"); copy.closest(".miner-identity").after(box); } }
  });
  return h("div", { class: "row miner-identity", "aria-label": "Miner identity" },
    h("span", { class: "pill", title: uid == null ? "Run everycli register-hotkey to look up the current UID" : "Registration last checked " + fmtTime(r.checkedAt) }, uid == null ? "UID not checked" : "UID " + uid),
    h("span", { class: "hotkey" }, h("code", { title: key, "aria-label": "Public hotkey " + key }, key.slice(0, 6) + "…" + key.slice(-6)), copy));
}
function reconnectButton() {
  return button("Reconnect profile", async () => { await run("reconnect", S.hotkeySel || {}); await refreshAll(); });
}
function workStatus() {
  if (!S.health) return ["Checking…", "Fetching worker status", ""];
  const health = value(S.health), checks = health?.checks || [];
  if (errorOf(S.health) || !health || !checks.some(c => c.name === "Accepting new work")) return ["Unknown", "Could not verify availability", "warn"];
  const accepting = checks.find(c => c.name === "Accepting new work");
  if (checks.some(c => c.name !== "Accepting new work" && checkState(c) === "FAIL")) return ["Needs attention", "See health checks below", "warn"];
  if (!accepting.ok) return ["Paused", "Not accepting new jobs", "warn"];
  if (health.ok !== true) return ["Needs attention", "See health checks below", "warn"];
  return health.activity?.activeJobs > 0 ? ["Working", "Processing jobs", "ok"] : ["Ready", "Accepting new jobs", "ok"];
}
function workStatusStat() { const [state, sub, tone] = workStatus(); return stat("Status", state, sub, tone, "pulse"); }
function statusHero() {
  const [state, sub, tone] = workStatus(), checks = value(S.health)?.checks || [];
  const passing = checks.filter(c => checkState(c) === "PASS").length;
  return h("section", { class: "status-hero " + (tone || "idle"), "aria-label": "Miner status" },
    h("a", { class: "hero-ring", href: "#/overview", title: checks.length ? passing + " of " + checks.length + " health checks pass" : "Health checks running",
      onclick: (e) => { e.preventDefault(); $("#overview-health")?.scrollIntoView({ behavior: "smooth", block: "start" }); } },
      h("span", { class: "ping", "aria-hidden": "true" }), h("span", { class: "ping", "aria-hidden": "true" }),
      ringMeter(checks.length ? passing / checks.length : null),
      checks.length ? h("span", { class: "ring-num" }, h("b", null, passing + "/" + checks.length), "checks") : h("span", { class: "ring-num" }, h("b", null, "…"), "checking")),
    h("div", { class: "hero-main" },
      h("div", { class: "eyebrow" }, (S.session?.network === "testnet" ? "Testnet · " : "") + "Subnet " + (S.session?.netuid ?? 117),
        S.session?.version ? h("span", { class: "eyebrow-ver" }, " · v" + S.session.version) : null),
      h("div", { class: "hero-title" }, displayName() || "Your miner"),
      h("div", { class: "hero-state" }, h("b", null, state), " · " + sub),
      minerIdentity()),
    h("div", { class: "hero-side", id: "hero-side", "data-shown": S.bootShown ? "1" : null }, ...heroSide()));
}
function ringMeter(frac) {
  const ns = "http://www.w3.org/2000/svg", svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 44 44"); svg.setAttribute("aria-hidden", "true"); svg.setAttribute("class", frac == null ? "ring spin" : "ring");
  for (const [cls, dash] of [["track", 100], ["fill", frac == null ? 22 : Math.round(Math.max(0, Math.min(1, frac)) * 100)]]) {
    const c = document.createElementNS(ns, "circle");
    for (const [k, v] of [["cx", 22], ["cy", 22], ["r", 18], ["pathLength", 100], ["class", cls], ["stroke-dasharray", dash + " 100"]]) c.setAttribute(k, v);
    svg.append(c);
  }
  return svg;
}
function pageOverview() {
  const o = S.overview;
  const root = h("div", null, h("h1", null, "Overview"),
    h("p", { class: "lede" }, "Live status of your miner. Refreshes every 30 seconds while this tab is open."));
  if (!o) return root;
  if (!o.initialized) {
    root.append(...[minerIdentity()].filter(Boolean));
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
  root.append(statusHero());
  const [phase, tone] = workerState();
  const activity = health?.activity || {};
  const activeOffers = checks.filter((c) => (c.name || "").startsWith("Model: ")).length;
  const offersStat = stat("Active offers", String(activeOffers), activeOffers ? "" : "Choose models to serve", activeOffers ? "ok" : "warn", "offers");
  if (Array.isArray(S.offers?.offers)) offersStat.append(h("div", { class: "sub" }, modelViewLink(S.offers.offers.filter(r => integratedModel(r) && !r.offered && r.catalogGroup !== "candidate" && !["removed", "deprecated"].includes(r.providerAvailability?.status)).length + " not offered", "not-offered")));
  root.append(h("div", { class: "grid overview-stats", "aria-label": "Miner overview summary" },
    workerStat(phase, tone),
    workStatusStat(),
    offersStat,
    stat("Jobs", activity.activeJobs == null ? "Unknown" : activity.activeJobs + " active", activity.acceptedJobs == null ? "Accepted count not reported" : activity.acceptedJobs.toLocaleString() + " accepted so far", "", "bolt")));
  if (S.workloads?.length) root.append(notice("warn", "A worker for this miner runs in your Phala account, but this computer doesn't manage it",
    h("div", null, "It was probably deployed from another computer. Keep managing it there, or delete or replace it from the ", h("a", { href: "#/worker" }, "Worker page"), ".")));
  if (o.deployment.needsReconcile) root.append(notice("warn", "An operation was interrupted", "Your last change didn't finish cleanly. Re-sync it from the Worker page before anything else; never deploy a second worker."));
  const healthCard = card("Health checks", [button("Refresh", recheckHealth)],
    S.health ? (errorOf(S.health) ? errorView(errorOf(S.health)) : healthList(checks)) : h("p", { class: "muted" }, "Checking…"),
    h("p", { class: "small muted" }, "All green means the current readiness checks passed. It does not guarantee traffic, provider funds or earnings."));
  healthCard.id = "overview-health"; root.append(healthCard);
  const jobs = activity.jobsByModel;
  const topCompleted = Math.max(1, ...(Array.isArray(jobs) ? jobs.map(j => Number.isSafeInteger(j.completed) ? j.completed : 0) : [0]));
  root.append(card("Jobs by model", [h("a", { class: "btn small", href: "#/jobs" }, "Individual job history"), h("span", { class: "small muted" }, "All time")],
    errorOf(S.health) ? errorView(errorOf(S.health)) : !Array.isArray(jobs)
      ? h("p", { class: "empty" }, "Per-model job counts have not been reported yet.")
      : jobs.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm" },
        h("thead", null, h("tr", null, ["Model", "Completed", "Active", "Failed attempts", "Cancelled / expired"].map(label => h("th", null, label)))),
        h("tbody", null, jobs.map(j => h("tr", null,
          h("td", { "data-label": "Model" }, h("b", null, S.offers?.offers?.find(r => r.model === j.model)?.name || j.model)),
          ...[["Completed", j.completed], ["Active", j.active], ["Failed attempts", j.failed], ["Cancelled / expired", j.cancelled]].map(([label, n]) => h("td", { "data-label": label, class: "num" },
            Number.isSafeInteger(n) ? n.toLocaleString() : "—", label === "Completed" && Number.isSafeInteger(n) ? meter(n / topCompleted) : null)))))))
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
  return r.remainingUsd > 0 && r.remainingUsd < 0.01 ? "<$0.01" : r.remainingUsd != null ? fmtUsd(r.remainingUsd) : "—";
}
function phalaCreditAlert(r) {
  if (r.provider !== "phala" || r.status !== "ok" || !Number.isFinite(r.remainingUsd) || r.remainingUsd > 0) return null;
  return r.stale
    ? "Phala's last reported credited balance was zero or negative. Refresh balances and check billing. Postpaid credit limits are not included."
    : "Phala has no credited funds. Add funds or check your postpaid billing arrangement. Postpaid credit limits are not included.";
}
function balanceSummary(rows) {
  if (!rows.length) return ["ok", "no keys to check"];
  const unsupported = rows.filter(r => r.status === "unsupported").length;
  const unavailable = rows.filter(r => r.status !== "unsupported" && (r.status !== "ok" || r.stale)).length;
  const alerts = rows.some(phalaCreditAlert);
  const details = [];
  if (unavailable) details.push(unavailable + " unavailable or stale");
  if (alerts) details.push("Phala credit alert");
  if (details.length) return ["warn", details.join(" · ")];
  if (rows.length > unsupported) details.push((rows.length - unsupported) + " checked");
  if (unsupported) details.push(unsupported + " without balance lookup");
  return ["ok", details.join(" · ")];
}
function balancesCard() {
  const v = value(S.balances), rows = [...(v?.balances || [])]
    .sort((a, b) => Number(a.status === "unsupported") - Number(b.status === "unsupported"));
  const panel = card("Provider balances", [button("Refresh", async () => { await loadBalances(); if (currentRoute() !== "keys") renderBalancePanel(); })],
    !S.balances ? h("p", { class: "muted" }, "Loading…") : errorOf(S.balances) ? errorView(errorOf(S.balances)) : rows.length
      ? h("div", { class: "table-wrap" }, h("table", null, h("thead", null, h("tr", null, h("th", null, "Provider"), h("th", null, "Remaining"), h("th", null, "Status"), h("th", null, "Checked"))),
        h("tbody", null, rows.map((r) => h("tr", null, h("td", null, providerName(r.provider)),
          h("td", null, balanceAmount(r)),
          h("td", null, h("span", { class: "badge " + (r.status === "unsupported" ? "" : r.status === "ok" && !r.stale && !phalaCreditAlert(r) ? "ok" : "warn") }, phalaCreditAlert(r) ? "Credit alert" : r.status || "—"), r.stale ? h("span", { class: "small muted" }, " stale") : null,
            phalaCreditAlert(r) ? h("div", { class: "notice warn", role: "alert" }, h("b", null, "Phala credit alert"),
              h("p", null, phalaCreditAlert(r)), h("a", { href: "https://cloud.phala.network/", target: "_blank", rel: "noopener noreferrer" }, "Check Phala billing")) : null,
            r.diagnostic ? r.status === "unsupported"
              ? h("div", { class: "small muted" }, h("p", null, r.diagnostic.message), h("p", null, r.diagnostic.next))
              : errorView(r.diagnostic) : null,
            r.provider === "google" && r.status === "unsupported" ? h("p", { class: "small" },
              h("a", { href: "https://aistudio.google.com/billing", target: "_blank", rel: "noopener noreferrer" }, "Google AI Studio billing"), " · ",
              h("a", { href: "https://docs.cloud.google.com/billing/docs/how-to/budgets", target: "_blank", rel: "noopener noreferrer" }, "Set up budget alerts")) : null),
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
async function loadJobDetail(row) {
  const request=++S.jobDetailRequest;S.jobDetail=null;S.jobDetailError=false;S.jobDetailBusy=true;
  try {
    const result=await run("jobs",{jobId:row.jobId,limit:"100"},{quiet:true});
    if(request!==S.jobDetailRequest||S.openAttempt!==row.attemptId)return;
    const rows=value(result)?.history?.rows;S.jobDetail=Array.isArray(rows)?rows.find(x=>x.attemptId===row.attemptId)??null:null;S.jobDetailError=result.state!=="done"||!Array.isArray(rows);
  } catch { if(request===S.jobDetailRequest&&S.openAttempt===row.attemptId)S.jobDetailError=true; }
  finally {if(request===S.jobDetailRequest&&S.openAttempt===row.attemptId){S.jobDetailBusy=false;if(currentRoute()==="jobs")render();}}
}
const PHASE_LABELS={queued:"Job queued",start_authorized:"Worker authorized to start",submitting:"Submitting to provider",submitted:"Provider request recorded",provider_queued:"Waiting in provider queue",provider_processing:"Provider generating",provider_waiting:"Waiting for provider status",provider_poll_retry:"Retrying provider status",provider_completed:"Provider reported completion",retrieving_output:"Retrieving output",output_fetch_retry:"Retrying output retrieval",uploading_output:"Uploading output",upload_retry:"Retrying output delivery",upload_received:"Output received",validating_output:"Verifying output",validation_busy:"Verification capacity busy",completion_error:"Output delivery or verification error",output_rejected:"Output rejected",output_accepted:"Output verified and accepted",worker_failed:"Worker reported failure",deadline_reached:"Job deadline reached"};
function timelineView(events) {
  const first = events.find(e => Number.isSafeInteger(e.at))?.at;
  const clock = t => new Date(t).toLocaleTimeString(undefined, { timeZone: "UTC", hourCycle: "h23", hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const elapsed = t => { const s = Math.round((t - first) / 1000); return s < 60 ? "+" + s + " s" : "+" + Math.floor(s / 60) + " min " + (s % 60) + " s"; };
  return h("div", null, h("h4", null, "Timeline"), first ? h("p", { class: "small muted" }, "Started " + reportDate(first) + ". Times are UTC.") : null,
    h("ol", { class: "job-timeline" }, events.map(e => h("li", null,
      h("span", { class: "mono small" }, Number.isSafeInteger(e.at) ? clock(e.at) : "—"),
      h("span", { class: "small muted" }, first && Number.isSafeInteger(e.at) ? elapsed(e.at) : ""),
      h("span", null, PHASE_LABELS[e.phase] || e.phase, e.code ? h("span", { class: "mono small muted" }, " " + e.code) : null, e.retries ? h("span", { class: "small muted" }, " · retries: " + e.retries) : null),
      h("span", { class: "small muted" }, e.source === "worker" ? "Worker" : "Coordinator")))));
}
function historyDetail(row) {
  if(S.jobDetail?.attemptId===row.attemptId)row=S.jobDetail;
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
    S.jobDetailBusy ? h("p",{role:"status"},"Loading job timeline…") : null,
    S.jobDetailError ? h("p",{role:"status",class:"muted"},"Timeline details are temporarily unavailable. Basic attempt facts remain available. Reopen details to retry.") : null,
    row.progress?.last ? h("p",null,"Last reported phase: "+(PHASE_LABELS[row.progress.last.phase]||row.progress.last.phase)+" · "+reportDate(row.progress.last.receivedAt)) : null,
    Array.isArray(row.progress?.timeline) && row.progress.timeline.length ? timelineView(row.progress.timeline) : null,
    !S.jobDetailBusy&&!S.jobDetailError&&!row.progress?.workerEvidence ? h("p",{class:"small muted"},"Detailed provider phases were not reported or retained for this attempt. Basic timestamps and recorded failure codes remain available.") : null,
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
        h("td", { "data-label": "Inspect" }, h("button", { class: "btn small", "aria-label": "View attempt " + row.attemptId, onclick: () => { S.openAttempt = row.attemptId; S.jobDetail=null; render(); loadJobDetail(row); $("#job-detail")?.focus({ preventScroll: true }); $("#job-detail")?.scrollIntoView({ block: "nearest", behavior: "smooth" }); } }, "Details"))
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
  terms: "Accept the Miner Terms",
  hotkey: "Select your hotkey", phala: "Add your Phala hosting token", providers: "Add a provider API key",
  init: "Create your miner profile", deploy: "Deploy your worker", activate: "Activate your keys",
  offers: "Choose models to serve", resume: "Start accepting work",
};
function stepDone(st) {
  if (st.id === "resume") return findCheck("Accepting new work")?.ok === true;
  if (st.id === "offers") {
    const bid = findCheck("Current model bid");
    return bid ? bid.ok : (value(S.health)?.checks || []).some((c) => (c.name || "").startsWith("Model: ") && checkState(c) === "PASS");
  }
  return !!st.done;
}
function setupCompleteNotice() {
  const health = value(S.health), problems = (health?.checks || []).filter(c => checkState(c) !== "PASS");
  return notice(health?.ok === true ? "ok" : "warn", "Setup complete", h("div", null,
    h("p", null, health?.ok === true ? "Your worker is ready to accept work. Jobs depend on customer demand and your offers."
      : "All setup steps are confirmed. Some health checks still need attention; completing setup does not mean every model is ready to serve."),
    problems.length ? h("details", null, h("summary", null, "Review " + problems.length + " health checks"), checksList(problems, true)) : null,
    h("a", { href: "#/overview" }, "View worker readiness")));
}
async function startAcceptingWork() {
  S.resumeResult = await run("resume");
  await refreshAll();
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
  if (steps.length && done === steps.length) { const n = setupCompleteNotice(); n.id = "setup-complete"; root.append(n); }
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
  else {
    const n = setupCompleteNotice(); n.id = "setup-complete";
    if ($("#setup-complete")) $("#setup-complete").replaceWith(n); else $("#setup-guide")?.append(n);
  }
}
async function refreshAll() {
  await loadOverview();
  if (!managedHere() && !S.removal) await loadWorkloads();
  if (S.overview?.initialized) await loadHealth();
  if (currentRoute() === "worker") await loadWorkerAuto();
  render();
}
function termsForm(o) {
  const t = o?.terms;
  if (!t) return h("p", null, "Loading Miner Terms…");
  // Blocks are separated by blank lines; a block's first line is its title.
  const blocks = String(t.text || "").trim().split(/\n\s*\n/).map(block => block.split("\n"));
  const paragraphs = blocks.map(([first, ...rest], i) => i === 0
    ? h("div", { class: "terms-title" }, h("h3", null, first), rest.length ? h("p", { class: "small muted" }, rest.join(" ")) : null)
    : rest.length ? h("section", null, h("h4", null, first), h("p", null, rest.join(" "))) : h("p", null, first));
  const review = h("details", { class: "terms-review", open: !t.accepted }, h("summary", null, "Read Miner Terms · " + t.termsVersion), h("div", { class: "terms-text" }, ...paragraphs));
  const identity = h("dl", { class: "kv small" }, h("dt", null, "Hotkey"), h("dd", null, t.hotkey || "Select and check your hotkey first"), h("dt", null, "Network"), h("dd", null, t.network));
  if (t.accepted) return h("div", { class: "stack" }, notice("ok", "Terms accepted on this device", "Version " + t.termsVersion + " for this hotkey. You remain responsible for required provider approvals and your commercial arrangements."), identity, review, h("div", { class: "row" }, continueButton("terms")));
  // Drafts live only in memory and are reset for a different identity or document.
  const scope = [t.termsVersion, t.documentSha256, t.network, t.hotkey].join(":");
  if (S.termsDraft !== scope) S.termsDraft = null;
  const checked = S.termsDraft === scope;
  const submit = h("button", { class: "btn primary", disabled: !checked || !t.hotkey, onclick: async () => {
    if (S.termsDraft !== scope || !t.hotkey) return;
    const job = await run("terms-accept", { acceptTerms: t.termsVersion, termsHotkey: t.hotkey, termsSha256: t.documentSha256 });
    if (job.state === "done") { S.termsDraft = null; S.openStep = null; }
    await refreshAll();
  } }, "Accept Miner Terms");
  const checkbox = h("input", { type: "checkbox", checked, disabled: !t.hotkey, "aria-label": "I accept the Miner Terms", onchange: e => {
    S.termsDraft = e.target.checked ? scope : null; submit.disabled = !e.target.checked || !t.hotkey;
  } });
  return h("div", { class: "stack" },
    h("p", null, "You must obtain and maintain all required provider permissions for resale and serving third-party requests, and contact providers directly for approvals and discounts. A working API key does not grant these rights."),
    identity, review,
    h("label", { class: "check" }, checkbox, h("span", null, "I accept Miner Terms " + t.termsVersion + " and confirm that I am responsible for obtaining required provider permissions and negotiating provider discounts directly.")),
    h("p", { class: "small muted" }, "Acceptance is recorded in this profile. This does not deploy a VM, change offers or submit a paid generation."), h("div", { class: "row" }, submit));
}
function stepBody(id) {
  const o = S.overview;
  switch (id) {
    case "hotkey": return stepHotkey(o);
    case "terms": return termsForm(o);
    case "phala": return h("div", null,
      h("p", null, "Your worker runs in a sealed, verifiable machine (a TEE) on Phala Cloud. Phala bills your own account for hosting, usually a few cents per hour."),
      h("ol", { class: "small" },
        h("li", null, "Sign in at ", h("a", { href: "https://cloud.phala.network", target: "_blank", rel: "noopener noreferrer" }, "cloud.phala.network"), "."),
        h("li", null, "Open Settings → API Tokens (not \"Private AI\") and create a token."),
        h("li", null, "Paste it below. It is saved only on this computer, in an owner-only file.")),
      keyForm("phala", refreshAll, "phala"),
      continueButton("phala"));
    case "providers": return h("div", null,
      h("p", null, "The worker generates with your own provider account, so each job is billed there. You must obtain required provider resale approvals and negotiate discounts directly. Add at least one key. You can add more later on the API keys page."),
      h("h3", null, "Direct providers"), setupProviders(true).map((p) => keyRow(p, "providers")),
      h("details", { open: setupProviders(false).some((p) => S.keyStatus[p]) },
        h("summary", null, "Other model providers"), setupProviders(false).map((p) => keyRow(p, "providers"))),
      continueButton("providers"));
    case "init": return h("div", null,
      h("p", null, "everycli proves you own the hotkey by signing a challenge on this computer, then downloads your signed worker configuration. No machine is created and nothing is paid."),
      S.hotkeySel ? h("p", { class: "small muted" }, "Using " + describeHotkey(S.hotkeySel) + ".") : o?.registration ? h("p", { class: "small muted" }, "Using the hotkey checked in step 1.") : null,
      o?.terms?.accepted !== true ? notice("warn", null, h("span", null, "Accept the Miner Terms for this hotkey first. ", h("a", { href: "#/setup", onclick: () => { S.openStep = "terms"; render(); } }, "Go to the Miner Terms step"))) : null,
      h("button", { class: "btn primary", disabled: o?.terms?.accepted !== true, onclick: async () => {
        const job = await run("init", S.hotkeySel || {});
        if (job.state === "done") S.openStep = null;
        await refreshAll();
      } }, "Create miner profile"));
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
      findCheck("Accepting new work")?.ok === true ? notice("ok", "Accepting work is enabled", "The coordinator confirmed that routing is enabled and this miner is not paused.")
        : errorOf(S.resumeResult) ? errorView(errorOf(S.resumeResult))
        : S.resumeResult?.state === "done" ? notice("warn", "Start request accepted", "The status check has not confirmed it yet. Refresh status to check without sending another start request.") : null,
      h("div", { class: "row" }, button("Start accepting work", startAcceptingWork, "primary"),
        button("Refresh status", async () => { await loadHealth(false); render(); }, "small")));
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
// `extra` (e.g. Remove) sits in the same row as Save/Test so the buttons line up.
function keyForm(provider, after, stayOn, extra = null) {
  const saved = (S.overview?.keys || []).find((k) => k.provider === provider)?.saved;
  const workspace = provider === "alibaba" ? h("input", { type: "text", placeholder: "Singapore workspace ID", "aria-label": "Alibaba workspace ID", autocomplete: "off" }) : null;
  const input = h("input", { type: "password", autocomplete: "off", spellcheck: "false", placeholder: saved ? "Paste to replace" : "Paste key", "aria-label": providerName(provider) + " key" });
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
    const r = await api("/api/keys", { provider, key, ...(workspace?.value.trim() ? { workspace: workspace.value.trim() } : {}) });
    const err = r.data.error;
    if (err?.error === "credential_rejected" && provider !== "fal-billing") err.message = providerName(provider) + " rejected this key, so it was not saved.";
    S.keyStatus[provider] = r.ok ? checkMessage(provider, r.data.check, true) : { kind: "error", error: err };
    await finish();
  }, "primary small");
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
    h("div", { class: "row" }, h("span", { class: "badge " + (saved ? "ok" : "") }, saved ? "SAVED" : "NOT SAVED"), workspace, input, save, test, extra),
    status);
  return form;
}
function setupProviders(direct) { return Object.keys(PROVIDERS).filter(p => p !== "phala" && !p.endsWith("-billing") && DIRECT_PROVIDERS.has(p) === direct).sort(compareProviders); }
function keyRow(provider, stayOn) {
  const [name, hint] = PROVIDERS[provider] || [provider, ""];
  return h("div", { class: "card flat", "data-provider-key": provider }, h("h3", null, name), h("p", { class: "small muted" }, hint), keyForm(provider, null, stayOn));
}
function providerKeyItem(k) {
  if (!k) return h("li");
  return h("li", { "data-provider-key": k.provider },
    h("div", null, h("b", null, providerName(k.provider)), h("div", { class: "small muted" }, (PROVIDERS[k.provider] || [, ""])[1])),
    keyForm(k.provider, null, null, k.saved ? button("Remove", async () => {
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
    const rows = keys.filter((k) => k.kind === kind).sort((a,b) => compareProviders(a.provider,b.provider));
    const group = card(title, null, h("ul", { class: "keylist" }, rows.map(providerKeyItem)));
    group.id = "keys-" + kind; root.append(group);
  }
  return root;
}

// ---------- Models ----------
function readiness(r) {
  const catalog = r.providerAvailability, worker = r.workerAvailability;
  if (catalog?.status === "removed") return ["Removed by provider", "bad"];
  if (catalog?.status === "deprecated") return ["Deprecated by provider", "bad"];
  const b = r.blockers || [];
  if (b.includes("pricing_required")) return ["Pricing required", "warn"];
  if (b.includes("worker_release_required")) return ["Worker update required", "warn"];
  if (b.includes("worker_offline")) return ["Worker offline", "bad"];
  if (b.includes("worker_capabilities_unknown")) return ["Worker check required", "warn"];
  if (b.includes("worker_provider_required")) return ["Add provider key to worker", "warn"];
  if (worker?.status === "access_denied") return ["Provider access unavailable for worker key", "bad"];
  if (worker?.status === "temporarily_unavailable") return ["Worker provider check needs refreshing", "warn"];
  if (catalog?.status === "access_denied") return ["Provider catalog could not be read", "warn"];
  if (catalog?.status === "temporarily_unavailable") return [
    typeof catalog.checkedAt === "number" && Date.now() - catalog.checkedAt > 3600000
      ? "Provider catalog check is stale" : "Provider catalog check temporarily unavailable", "warn"];
  if (r.workerAvailability?.status === "unchecked" && r.offerable === true) return r.provider === "fal"
    ? ["Available · no per-model check", ""] : ["Available · account access not checked", "warn"];
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
  const seg = h("div", { class: "seg model-tabs" }, OFFER_FILTERS.map(([f, label]) => h("button", { disabled: S.batchBusy, class: S.offerFilter === f ? "on" : "", onclick: () => openModels(f, S.modelProvider, false) }, label,
    f === "all" && S.newModels.size ? h("span", { class: "badge info", "aria-label": S.newModels.size + " new models" }, S.newModels.size + " new") : null)));
  const search = h("input", { type: "search", disabled: S.batchBusy, value: S.modelQuery, placeholder: "Search models", "aria-label": "Search models", oninput: e => { S.modelQuery = e.target.value; patchModels(); } });
  const providers = [...new Set((S.offers.offers || []).map(r => r.provider).filter(Boolean))].sort(compareProviders);
  const provider = h("select", { "aria-label": "Filter model provider", disabled: S.batchBusy, onchange: e => { S.modelProvider = e.target.value; patchModels(); } },
    h("option", { value: "" }, "All providers"), providers.map(p => h("option", { value: p }, providerName(p)))); provider.value = S.modelProvider;
  const kind = h("select", { "aria-label": "Filter model type", disabled: S.batchBusy, onchange: e => { S.modelKind = e.target.value; patchModels(); } },
    h("option", { value: "" }, "All types"), ["video", "image", "audio"].map(v => h("option", { value: v }, v[0].toUpperCase() + v.slice(1)))); kind.value = S.modelKind;
  if (S.overview?.initialized) root.append(providerOffersCard(), automaticOffersCard());
  if (S.offerFilter === "not-offered") root.append(notice("", "Models without an active offer", "New models start at 0% discount. Review a model and turn its offer on when you want to serve it."));
  if (S.offerFilter === "candidate") root.append(notice("", "Not available for new offers", "These models need attention before they can be offered. Existing offers can still be withdrawn."));
  const body = h("div", { id: "model-list" });
  const offersCard = card("Offers", [seg, refresh], h("div", { class: "row model-filters" }, search, provider, kind), body);
  offersCard.id = "offers-table"; root.append(offersCard);
  fillModels(body);
  return root;
}
function visibleOffers() {
  let rows = (S.offers?.offers || []).filter(integratedModel);
  if (S.overview?.initialized && S.offerFilter === "active") rows = rows.filter(r => r.offered);
  if (S.offerFilter === "not-offered") rows = rows.filter(r => !r.offered && r.catalogGroup !== "candidate" && !["removed", "deprecated"].includes(r.providerAvailability?.status));
  if (S.overview?.initialized && S.offerFilter === "ready") rows = rows.filter(r => canApplyOffer(r) && readiness(r)[0].startsWith("Available") && r.localKeyConfigured !== false);
  if (S.offerFilter === "all" || (!S.overview?.initialized && S.offerFilter === "ready")) rows = rows.filter(r => r.catalogGroup !== "candidate" && r.providerAvailability?.status !== "removed");
  if (S.offerFilter === "candidate") rows = rows.filter(r => r.catalogGroup === "candidate" || ["removed", "deprecated"].includes(r.providerAvailability?.status));
  const q = S.modelQuery.trim().toLowerCase();
  return rows.filter(r => (!S.modelProvider || r.provider === S.modelProvider) && (!S.modelKind || (r.kind || r.contract?.kind) === S.modelKind)
    && (!q || [r.name, r.model, r.providerApiId, providerName(r.provider)].filter(Boolean).join(" ").toLowerCase().includes(q)))
    .sort((a,b) => compareProviders(a.provider,b.provider));
}
function fillModels(body) {
  const rows = visibleOffers();
  body.replaceChildren(h("p", { class: "small muted" }, rows.length + (rows.length === 1 ? " model shown" : " models shown")),
    S.overview?.initialized ? batchToolbar(rows) : h("span"),
    S.batchReport ? batchReportView() : h("span"),
    rows.length ? h("div", { class: "table-wrap" }, h("table", { class: "cards-sm" },
      h("thead", null, h("tr", null, ["Model", "Provider", "Readiness", "Your offer", "Base reward", "Change"].map(t => h("th", null, t)))),
      h("tbody", null, rows.map(offerRow)))) : h("p", { class: "empty" }, "No models match these filters. Try All models or clear the search."),
    h("p", { class: "small muted" }, "Base reward is a reference value per job; actual jobs are repriced for their settings. Offers do not guarantee traffic or profit."));
  if (S.batchBusy) body.querySelectorAll("button, input").forEach(el => { el.disabled = true; });
}
function patchModels() { const body = $("#model-list"); if (body) fillModels(body); }
function providerOffersCard() {
  const rows = (S.offers?.offers || []).filter(integratedModel);
  const providers = [...new Set(rows.map(r => r.provider).filter(Boolean))].sort(compareProviders);
  const controls = providers.map(provider => {
    const models = rows.filter(r => r.provider === provider), active = models.filter(r => r.offered);
    const inactive = models.filter(r => !r.offered && r.catalogGroup !== "candidate" && !["removed", "deprecated"].includes(r.providerAvailability?.status));
    const eligible = inactive.filter(r => canApplyOffer(r) && r.localKeyConfigured !== false);
    const on = active.length > 0 || models.some(r => r.automaticOffer?.managed);
    const toggle = h("button", { type: "button", role: "switch", "aria-checked": String(on), "aria-label": providerName(provider) + " offers",
      class: "offer-switch" + (on ? " on" : ""), disabled: S.batchBusy || S.pendingOperations.size > 0 || (!on && !eligible.length),
      title: on ? "Turn off all offers for " + providerName(provider) : "Turn on eligible offers for " + providerName(provider),
      onclick: () => changeProviderOffers(provider, !on) }, h("span", { class: "switch-track", "aria-hidden": "true" }, h("span")), on ? "On" : "Off");
    return h("div", { class: "provider-offer", "data-offer-provider": provider },
      h("div", { class: "row" }, h("b", null, providerName(provider)), toggle),
      h("div", { class: "small" }, modelViewLink(active.length + " offered", "active", provider), " · ", modelViewLink(inactive.length + " not offered", "not-offered", provider)),
      h("div", { class: "small muted" }, on ? "Turn off to withdraw this provider's offers." : eligible.length ? eligible.length + (eligible.length === 1 ? " model can" : " models can") + " be turned on." : "No models ready to offer. Check the provider key and model readiness."));
  });
  return card("Provider offers", [], h("p", { class: "small muted" }, "Switches apply to all models for that provider, across filters. Turning on keeps saved discounts and uses 0% for new models. Turning off keeps discounts for later and lets current jobs finish."), h("div", { class: "provider-offers" }, controls));
}
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
async function reviewOfferBatch(params, title, epoch) {
  const prepared = await run("offer-batch", params, { quiet: true, prepareOnly: true });
  if (prepared.state !== "confirm") return prepared;
  const yes = epoch === S.navigationEpoch && await confirmDialog(prepared.result.question, title);
  return yes && epoch === S.navigationEpoch
    ? run("offer-batch", params, { quiet: true, approvals: [prepared.result.hash], exactApproval: true })
    : { state: "cancelled" };
}
async function changeProviderOffers(provider, enabled) {
  if (S.batchBusy || S.pendingOperations.size) return;
  const epoch = S.navigationEpoch;
  S.batchBusy = true; clearBatchReport(); render();
  const report = [];
  try {
    await loadOffers(false);
    if (S.offersError || !Array.isArray(S.offers?.offers)) throw Error("Could not refresh offers. No provider offers were changed.");
    const rows = (S.offers?.offers || []).filter(r => integratedModel(r) && r.provider === provider);
    const changes = rows.filter(r => enabled ? !r.offered && canApplyOffer(r) && r.localKeyConfigured !== false : r.offered || r.automaticOffer?.managed);
    if (enabled) for (const row of rows.filter(r => !r.offered && !changes.includes(r))) report.push({ model: row.name || row.model, ok: false, skipped: true,
      message: "Skipped: " + (row.localKeyConfigured === false ? "No provider key saved." : offerBlocker(row) || "Not ready to offer.") });
    if (changes.length) {
      const params = { models: changes.map(r => r.model).join(","), ...(enabled ? { keepDiscounts: true } : { withdraw: true }) };
      const job = await reviewOfferBatch(params, (enabled ? "Turn on " : "Turn off ") + providerName(provider) + " offers", epoch);
      const ok = job.state === "done";
      const message = ok ? enabled ? "Offer on; saved discount kept (0% for a new model)." : "Offer off; discount kept."
        : job.state === "confirm" ? "Offers changed; nothing applied. Review again."
        : job.state === "cancelled" ? "Cancelled; unchanged."
        : errorOf(job)?.message || "Result not confirmed. Refresh offers before retrying.";
      for (const row of changes) {
        report.push({ model: row.model, ok, message });
        if (ok) { S.selectedOffers.delete(row.model); S.offerDrafts.delete(row.model); }
      }
    } else if (!report.length) report.push({ model: providerName(provider), ok: true, message: enabled ? "All eligible offers are already on." : "All offers are already off." });
  } catch (e) { report.push({ model: providerName(provider), ok: false, message: e.message || "Result not confirmed. Refresh offers before retrying." });
  } finally {
    showBatchReport(report, epoch); S.batchBusy = false;
    await loadOffers(); await loadHealth();
    if (epoch === S.navigationEpoch && currentRoute() === "models") render(); else renderNav();
  }
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
      const job = await reviewOfferBatch(params, "Batch model offers", epoch);
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
function nativeCapabilityDetails(r) {
  const c = r.contract?.nativeCapabilities;
  if (!c) return null;
  const parts = [];
  if (c.resolutions?.length) parts.push("Sizes: " + c.resolutions.join(", "));
  if (c.aspects?.length) parts.push("Aspect ratios: " + c.aspects.join(", "));
  if (c.qualities?.length) parts.push("Quality: " + c.qualities.join(", "));
  if (c.formats?.length || c.codecs?.length) parts.push("Formats: " + (c.formats || c.codecs).join(", "));
  if (c.tasks?.length) parts.push("Tasks: " + c.tasks.join(", "));
  if (c.maxReferences) parts.push("Reference inputs: up to " + c.maxReferences);
  if (c.count) parts.push("Images per request: " + c.count.join("–"));
  if (c.keyframes) parts.push("Keyframes: up to " + c.keyframes);
  if (c.referenceVoices) parts.push("Reference voices: up to " + c.referenceVoices);
  if (c.mask) parts.push("Masked image edits supported");
  return h("div", { class: "small muted" }, ...parts.map(v => h("div", null, v)));
}
function googleCapabilityDetails(r) {
  const c = r.contract?.googleCapabilities;
  if (!c) return null;
  const parts = [];
  if (c.resolutions) parts.push("Sizes: " + c.resolutions.join(", "));
  if (c.aspects) parts.push("Aspect ratios: " + c.aspects.join(", "));
  if (c.durationRange) parts.push("Video duration: " + c.durationRange.join("–") + " seconds; extension up to 40 seconds");
  if (c.imageReferences) parts.push("Image references: up to " + c.imageReferences);
  if (c.videoReferences) parts.push("Video references supported");
  if (c.documentReferences) parts.push("PDF references supported");
  if (c.tasks) parts.push("Video generation, first/last frames, editing and extension");
  if (c.formats) parts.push("Audio formats: " + c.formats.map(v => v.replace("audio/", "")).join(", "));
  if (c.speech) parts.push("Voice and language selection, custom voices, and two-speaker dialogue");
  if (c.search) parts.push("Google Search grounding supported");
  return h("div", { class: "small muted" }, ...parts.map(v => h("div", null, v)));
}
function offerRow(r) {
  const [ready, tone] = readiness(r);
  const disc = h("input", { type: "number", min: "0", max: "99.9", step: "0.01", value: S.offerDrafts.get(r.model) ?? (r.discountBp != null ? String(r.discountBp / 100) : "0"), oninput: e => S.offerDrafts.set(r.model, e.target.value), class: "tiny", "aria-label": "Discount percent for " + (r.name || r.model) });
  const baseMicrousd = r.baseMinerRewardMicrousd ?? r.pricing?.minerRewardMicrousd;
  const base = typeof baseMicrousd === "number" ? baseMicrousd / 1e6 : null;
  const discountLabel = ((r.discountBp || 0) / 100).toFixed(2) + "% off";
  const yours = r.automaticOffer?.managed ? h("span", { class: "badge " + (r.automaticOffer.state === "active" ? "info" : "warn"), title: r.automaticOffer.reason ? automaticReason(r.automaticOffer.reason) : "Uses verified price reports and your policy limits" }, r.automaticOffer.state === "active" ? "Automatic · " + discountLabel : "Automatic paused") : r.offered ? h("span", { class: "badge " + (r.pricingCurrent ? r.discountBp > 0 ? "ok" : "neutral" : "warn") }, r.pricingCurrent ? "Active · " + discountLabel : "Stale pricing")
    : modelViewLink("Not offered · " + discountLabel, "not-offered", "", { class: "badge neutral", title: "Show models without an active offer" });
  const apply = button(r.offered ? "Update" : "Offer", async () => { const job = await run("offer", { model: r.model, discountPct: disc.value }); if (job.state === "done") S.offerDrafts.delete(r.model); await loadOffers(); await loadHealth(); render(); }, "small primary");
  apply.disabled = !canApplyOffer(r); disc.disabled = !canApplyOffer(r);
  return h("tr", { "data-model": r.model },
    h("td", { "data-label": "Model" }, h("label", { class: "row" }, S.overview?.initialized ? h("input", { type: "checkbox", checked: S.selectedOffers.has(r.model), disabled: S.batchBusy || !selectableOffer(r), "aria-label": "Select " + (r.name || r.model), onchange: e => { if (e.target.checked) S.selectedOffers.add(r.model); else S.selectedOffers.delete(r.model); patchModels(); } }) : null, h("b", null, r.name || r.model)), h("details", { class: "model-details" }, h("summary", null, "Model details"), h("span", { class: "id" }, "EveryFrame contract: " + r.model), h("span", { class: "id" }, "Provider API ID: " + (r.providerApiId || "Not reported")), googleCapabilityDetails(r), nativeCapabilityDetails(r))),
    h("td", { "data-label": "Provider" }, providerName(r.provider || ""), r.localKeyConfigured === false ? h("div", { class: "small muted" }, "No key saved") : null),
    h("td", { "data-label": "Readiness" }, h("span", { class: "badge " + tone }, ready)),
    h("td", { "data-label": "Your offer" }, yours),
    h("td", { "data-label": "Base reward" }, base != null ? fmtUsd(base, base < 1 ? 3 : 2) : "—"),
    h("td", { "data-label": "Change" }, !S.overview?.initialized ? h("a", { href: "#/setup" }, "Set up to offer") : h("div", { class: "row nowrap" }, disc, h("span", { class: "small muted" }, "% off"),
      apply,
      r.offered || r.automaticOffer?.managed ? button("Withdraw", async () => { await run("offer", { model: r.model, withdraw: true }); await loadOffers(); await loadHealth(); render(); }, "small") : null)));
}

// ---------- Automatic provider-price offers (explicit opt-in) ----------
function automaticReason(code) {
  return ({ automatic_offer_credential_changed: "Worker key differs from the reviewed account; apply the intended key or review this policy again", automatic_offer_price_unavailable: "A current provider price could not be verified", automatic_offer_price_stale: "Price check expired; new work paused",
    automatic_offer_price_placeholder: "Provider publishes a placeholder price; keep this model on manual offers", provider_price_rate_limited: "Provider rate-limited the price check; wait before retrying", provider_price_timeout: "Provider price check timed out", provider_price_access_denied: "Provider denied the price lookup; check this key's permissions", provider_key_required: "Save the provider key before checking its prices", invalid_provider_price: "Provider returned an unusable price", invalid_price_plan: "The selected price-check plan is unsupported",
    automatic_offer_price_incomplete: "Not all billing components have verified prices", automatic_offer_unit_unsupported: "Provider billing unit needs review",
    automatic_offer_pricing_unsupported: "Automatic pricing is not available for this contract", automatic_offer_cost_above_work_value: "Waiting for a lower provider price",
    automatic_offer_job_limit: "Estimated job cost exceeds your limit", automatic_offer_daily_limit: "Rolling exposure limit reached",
    automatic_offer_pricing_changed: "Contract pricing changed; review the policy again", automatic_offer_disabled: "Automatic offers are off",
    automatic_offer_discount_limit: "Offer exceeds your maximum discount", provider_model_unavailable: "Model retired by provider" })[code] || "Price check unavailable; new work paused";
}
async function loadAutomaticOffers() {
  if (S.autoBusy) return;
  const request = ++S.autoRequest;
  S.autoBusy = true;
  try {
    const job = await run("auto-offers-status", {}, { quiet: true });
    if (request === S.autoRequest) { S.autoOffers = value(job); S.autoError = errorOf(job); }
  } finally { if (request === S.autoRequest) S.autoBusy = false; }
}
function automaticDraft(provider, policy) {
  if (!S.autoDrafts.has(provider)) S.autoDrafts.set(provider, {
    models: new Set((policy?.models || []).map(m => m.model)), maxDiscountPct: policy ? String(policy.maxDiscountBp / 100) : "",
    bufferPct: policy ? String(policy.bufferBp / 100) : "20", maxDailyUsd: policy ? String(policy.dailyCapMicrousd / 1e6) : "",
    maxJobUsd: policy ? String(policy.jobCapMicrousd / 1e6) : "", preview: null,
  });
  return S.autoDrafts.get(provider);
}
// Up to six decimals for sub-cent prices, without trailing zeros.
function autoUsd(micro) {
  if (typeof micro !== "number" || !isFinite(micro)) return "—";
  const usd = micro / 1e6;
  return "$" + (usd >= 1 || usd === 0 ? usd.toFixed(2) : usd.toFixed(6).replace(/0+$/, "").replace(/\.(\d)$/, ".$10"));
}
// What "Preview prices" and "Review and enable" need, worded for the "To continue, …" notice.
const AUTO_REQUIRED = { models: "select at least one model", maxDiscountPct: "enter a maximum discount (0% is allowed)",
  maxDailyUsd: "enter a rolling 24-hour USD limit", maxJobUsd: "enter a per-job USD limit" };
function joinAnd(xs) { return xs.length < 2 ? xs.join("") : xs.slice(0, -1).join(", ") + " and " + xs[xs.length - 1]; }
function automaticOffersCard() {
  if (!S.autoTried) { S.autoTried = true; loadAutomaticOffers().then(() => { if (currentRoute() === "models") render(); }); }
  const refresh = button(S.autoOffers ? "Refresh automatic offers" : "Configure automatic offers", async () => { await loadAutomaticOffers(); if (currentRoute() === "models") render(); }, "small");
  refresh.disabled = S.autoBusy || S.batchBusy;
  const body = h("div", { class: "stack" }, h("p", { class: "small muted" },
    "Off by default. Your worker can check provider prices and adjust only the models you select. Set a maximum discount, cost buffer and estimated exposure limits. It keeps running when this dashboard is closed. Work value is not a cash payout or guaranteed profit."));
  if (S.autoError) body.append(errorView(S.autoError));
  if (!S.autoOffers?.automaticOffers) body.append(h("p", { class: "small muted" }, S.autoBusy ? "Loading policies…" : "Configure to review eligible Fal and OpenRouter models. Other providers and incomplete billing units keep manual offers."));
  else for (const p of S.autoOffers.automaticOffers.providers || []) {
    const provider = p.provider, draft = automaticDraft(provider, p.policy);
    const rows = (S.autoOffers.catalog || []).filter(m => m.provider === provider);
    const local = new Map((S.offers?.offers || []).map(m => [m.model, m]));
    const supported = rows.filter(m => m.supported);
    const ready = supported.filter(m => local.get(m.model)?.localKeyConfigured !== false);
    const busy = S.autoBusy || S.batchBusy || S.pendingOperations.size > 0;
    const details = h("details", { class: "automatic-provider", "data-automatic-provider": provider, open: S.autoExpanded.has(provider), ontoggle: e => {
      if (e.currentTarget.open) S.autoExpanded.add(provider); else S.autoExpanded.delete(provider);
    } }, h("summary", null, providerName(provider), " · ", p.enabled ? "Monitoring on" : "Off"));
    details.append(h("p", { class: "small muted" }, autoUsd(p.estimatedExposureMicrousd) + " estimated exposure in the rolling 24-hour window. Limits cover only this policy; manual offers, other account use and hosting are excluded."),
      h("p", { class: "small muted" }, provider === "fal" ? "Uses your deployed generation key's unit-price estimate. Jobs with unquoted billing components pause. No billing or admin key is sent to the worker." : "Uses OpenRouter's public video price catalog. Private account discounts are not assumed."));
    const list = h("div", { class: "automatic-models", role: "group", "aria-label": providerName(provider) + " automatic models" });
    for (const row of supported) {
      const available = local.get(row.model)?.localKeyConfigured !== false;
      list.append(h("label", { class: "row small" }, h("input", { type: "checkbox", checked: draft.models.has(row.model), disabled: busy || !available,
        "aria-label": "Automate " + row.model, onchange: e => { if (e.target.checked) draft.models.add(row.model); else draft.models.delete(row.model); draft.missing?.delete("models"); draft.preview = null; } }),
        local.get(row.model)?.name || row.model, available ? null : h("span", { class: "muted" }, "No local key saved")));
    }
    const select = button("Select eligible models", () => { draft.models = new Set(ready.map(m => m.model)); draft.preview = null; render(); }, "small");
    select.disabled = busy || !ready.length;
    details.append(h("div", { class: "row" }, select, h("span", { class: "small muted" }, (rows.length - supported.length) + (rows.length - supported.length === 1 ? " contract needs" : " contracts need") + " complete pricing before automation.")), list);
    const fields = h("div", { class: "automatic-limits" });
    for (const [key, label, max, step] of [["maxDiscountPct", "Maximum discount %", "99.9", "0.01"], ["bufferPct", "Cost buffer %", "1000", "0.01"], ["maxDailyUsd", "Rolling 24-hour estimated USD limit", "1000000", "0.000001"], ["maxJobUsd", "Per-job estimated USD limit", "1000000", "0.000001"]]) {
      fields.append(h("label", { class: "stack small" }, label, h("input", { type: "number", min: "0", max, step, value: draft[key], disabled: busy,
        placeholder: AUTO_REQUIRED[key] ? "Required" : null, "aria-invalid": draft.missing?.has(key) ? "true" : null,
        "aria-label": providerName(provider) + " " + label, oninput: e => { draft[key] = e.target.value; draft.missing?.delete(key); e.target.removeAttribute("aria-invalid"); draft.preview = null; } })));
    }
    details.append(fields);
    if (draft.missing?.size) details.append(h("div", { class: "notice warn", role: "alert" }, "To continue, " + joinAnd([...draft.missing].map((k, i, ks) => i && AUTO_REQUIRED[ks[i - 1]].startsWith("enter ") ? AUTO_REQUIRED[k].replace(/^enter /, "") : AUTO_REQUIRED[k])) + "."));
    const preview = button("Preview prices", () => changeAutomaticOffers(provider, "preview"), "small");
    const enable = button(p.enabled ? "Review policy changes" : "Review and enable", () => changeAutomaticOffers(provider, "enable"), "primary small");
    const disable = button("Disable automatic offers", () => changeAutomaticOffers(provider, "disable"), "small");
    preview.disabled = enable.disabled = busy || !ready.length; disable.disabled = busy || !p.enabled;
    details.append(h("div", { class: "row" }, preview, enable, disable), h("p", { class: "small muted" }, "A compatible worker with fresh price reports is required. Disabling withdraws managed offers. Manual edits remove a model from automation. Started jobs keep their agreed price."));
    if (draft.preview) details.append(h("div", { "data-automatic-preview": provider }, h("h4", null, "Price preview"), draft.preview.error ? errorView(draft.preview.error) : h("ul", { class: "small" }, (draft.preview.rows || []).map(r => h("li", null,
      (local.get(r.model)?.name || r.model) + ": " + (r.status === "ready" ? "Ready · " + (r.discountBp / 100).toFixed(2) + "% off" : automaticReason(r.reason)),
      typeof r.estimatedCostMicrousd === "number" ? " · estimated " + autoUsd(r.estimatedCostMicrousd) + " per job" : "", r.checkedAt ? " · checked " + ago(r.checkedAt) : "")))));
    if (p.models?.length) details.append(h("div", null, h("h4", null, "Current state"), h("ul", { class: "small" }, p.models.map(m => h("li", null, (local.get(m.model)?.name || m.model) + ": " + (m.state === "active" ? "Automatic offer active" : automaticReason(m.reason)))))));
    if (p.history?.length) details.append(h("details", null, h("summary", null, "Recent automatic offer changes"), h("ul", { class: "small" }, p.history.map(e => h("li", null, fmtTime(e.at) + " · " + (e.model || providerName(provider)) + " · " + (e.reason ? automaticReason(e.reason) : e.action))))));
    body.append(details);
  }
  return card("Automatic offers", [refresh], body);
}
async function changeAutomaticOffers(provider, action) {
  if (S.autoBusy || S.batchBusy || S.pendingOperations.size) return;
  const p = S.autoOffers?.automaticOffers?.providers?.find(p => p.provider === provider), draft = automaticDraft(provider, p?.policy);
  const params = action === "disable" ? { provider } : { provider, models: [...draft.models].sort().join(","), maxDiscountPct: draft.maxDiscountPct, bufferPct: draft.bufferPct, maxDailyUsd: draft.maxDailyUsd, maxJobUsd: draft.maxJobUsd };
  draft.missing = new Set(action === "disable" ? [] : Object.keys(AUTO_REQUIRED).filter(k => !String(params[k] ?? "").trim()));
  if (draft.missing.size) { draft.preview = null; render(); return; }
  const epoch = S.navigationEpoch;
  S.autoBusy = true; render();
  try {
    const job = await run("auto-offers-" + action, params, { quiet: action === "preview" });
    if (action === "preview") draft.preview = errorOf(job) ? { error: errorOf(job) } : value(job)?.preview;
    else if (job.state === "done") { S.autoDrafts.delete(provider); await loadOffers(); }
    else if (action === "enable" && [errorOf(job)?.error, errorOf(job)?.code].includes("automatic_offer_preview_blocked")) {
      const issues = errorOf(job)?.automaticOfferIssues;
      if (Array.isArray(issues) && issues.length) draft.preview = { rows: issues };
      else {
        // Compatibility with older CLIs that did not retain their failed preview.
        const check = await run("auto-offers-preview", params, { quiet: true });
        draft.preview = errorOf(check) ? { error: errorOf(check) } : value(check)?.preview;
      }
    }
  } finally {
    S.autoBusy = false;
    if (action !== "preview") await loadAutomaticOffers();
    if (epoch === S.navigationEpoch && currentRoute() === "models") render();
  }
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
  patchWorkerUpdate();
}
function patchWorkerUpdate() {
  if (currentRoute() === "worker") $("#worker-software")?.replaceWith(workerUpdateCard());
}
async function checkWorkerUpdate(quiet = false) {
  if (S.workerUpdateBusy || S.workerUpdateRunning || !managedHere()) return;
  S.workerUpdateBusy = true; patchWorkerUpdate();
  try { S.workerUpdate = await run("worker-update-check", {}, { quiet }); S.workerUpdateAt = Date.now(); }
  finally { S.workerUpdateBusy = false; patchWorkerUpdate(); }
}
async function changeWorkerUpdate(abort = false) {
  if (S.workerUpdateBusy || S.workerUpdateRunning) return;
  S.workerUpdateRunning = true; patchWorkerUpdate();
  try {
    S.workerUpdate = await run(abort ? "worker-update-abort" : "worker-update");
    S.workerUpdateAt = Date.now();
    await refreshAll();
  } finally { S.workerUpdateRunning = false; patchWorkerUpdate(); }
}
async function loadWorkerTools() {
  await loadWorkerAuto();
  if (!S.workerUpdate || Date.now() - S.workerUpdateAt > 60000) await checkWorkerUpdate(true);
}
function workerUpdateCard() {
  const v = value(S.workerUpdate), phase = v?.state, controls = workerControls();
  const busy = S.workerUpdateBusy || S.workerUpdateRunning, uncertain = S.pendingOperations.size > 0;
  const waiting = phase === "waiting" || phase === "release_expired";
  const check = button("Check for worker update", () => checkWorkerUpdate());
  check.disabled = busy || !managedHere() || uncertain;
  const update = button(phase === "waiting" ? "Continue worker update" : "Update worker", () => changeWorkerUpdate(), "primary");
  update.disabled = busy || uncertain || !controls.exists || automaticMode() !== false
    || !(phase === "available" && !controls.pending || phase === "waiting");
  const cancel = waiting ? button("Cancel worker update", () => changeWorkerUpdate(true), "small") : null;
  if (cancel) cancel.disabled = busy || uncertain || !controls.exists;
  const labels = { available: "Worker update available", current: "Worker is up to date", complete: "Worker update complete", waiting: "Worker update needs to continue", release_expired: "Worker update needs attention", aborted: "Worker update cancelled", unavailable: "No compatible worker update" };
  const panel = card("Worker software", null,
    h("p", { class: "small muted" }, "Installed release: ", h("span", { class: "wrap mono" }, S.overview?.profile?.release || "Not reported")),
    h("div", { class: "worker-update-result", "aria-live": "polite" },
      busy ? h("p", null, S.workerUpdateRunning ? "Updating worker… Follow the progress and confirmations below." : "Checking for a compatible signed update…")
        : S.workerUpdate?.state === "unknown" ? notice("warn", "Update result is unconfirmed", "Check the existing operation above before continuing.")
        : errorOf(S.workerUpdate) ? errorView(errorOf(S.workerUpdate))
        : h("div", { class: "stack" }, h("b", null, labels[phase] || "Check for a worker update"),
          v?.release && phase === "available" ? h("p", { class: "small wrap mono" }, "Available: " + v.release) : null,
          v?.reason ? h("p", { class: "small muted" }, v.reason) : null,
          phase === "waiting" ? h("p", { class: "small muted" }, "Progress is saved. Continue this update on the same VM, or cancel after reviewing its current state.") : null)),
    h("p", { class: "small muted" }, "Updates the existing VM, drains active jobs, waits for attestation and restores its previous accepting or paused state. You review the CLI's confirmations before anything changes."),
    automaticMode() === true ? notice("", "Automatic updates are enabled", h("div", null,
      "Turn off automatic updates below before starting a manual update or provider migration. ",
      button("Turn off automatic updates", async () => { await run("worker-auto-disable"); await loadWorkerTools(); }, "small"))) : null,
    h("div", { class: "row" }, check, update, cancel));
  panel.id = "worker-software"; return panel;
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
    : [["Installed version", v.installedVersion || v.currentVersion || S.session?.version], ["Latest version", v.latestVersion || v.latest]]
      .map(([k, ver]) => [k, typeof ver === "string" ? ver.replace(/^v/, "") : ver]);
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
  else if (!managedHere()) root.append(notice("", "No worker deployed from this computer", h("div", null, "Deploy one from the worker deployment step of the ", h("a", { href: "#/setup" }, "setup guide"), ".")));
  root.append(h("div", { class: "grid" },
    workerStateSummary(),
    nameStat(),
    stat("Compute price", d.computeHourlyUsd ? "$" + d.computeHourlyUsd + "/h" : "—", "Storage extra")));
  if (d.needsReconcile) root.append(notice("warn", "Confirm worker state", h("div", null,
    h("p", null, "A lifecycle operation is pending. Re-sync checks its result at Phala and the coordinator without repeating the cloud operation."),
    button("Re-sync operation", async () => { await run("reconcile"); await refreshAll(); }, "primary"))));
  root.append(workerUpdateCard(), workerAutoCard());
  const ph = d.phase || "not_deployed", controls = workerControls();
  const when = (ok, el, why) => { if (!ok) { el.disabled = true; el.title = why; } return el; };
  const startLimit = computeLimitInput("Maximum compute USD per hour");
  const actions = [
    ["Reconnect profile", "Prove ownership with the same saved hotkey and refresh local authorization. Keeps keys and history; does not deploy or change the VM.", reconnectButton()],
    ["Re-sync state", "Confirm completed starts, shutdowns or interrupted operations on this same VM.",
      when(!!d.appId, button("Re-sync state", async () => { await run("reconcile"); await refreshAll(); }), "No deployment recorded.")],
    ["Start accepting work", "Lets the network route paid jobs to this worker again.",
      when(controls.resume,
        button("Resume", startAcceptingWork, "primary"), "Already accepting work, or the worker isn't ready.")],
    ["Pause new work", "Finishes current jobs, then stops taking new ones. The machine keeps running and billing.",
      when(controls.pause, button("Pause", async () => { await run("stop", { drainOnly: true }, { label: "Pause new work" }); await refreshAll(); }), "Only a running worker can be paused.")],
    ["Apply saved keys", "Pushes keys you changed on the API keys page to this worker (pauses, restarts, re-attests).",
      when(controls.apply, button("Apply keys", async () => { await run("apply-api-keys"); await refreshAll(); }), "No running worker.")],
    ["Shut down", "Drains work, then shuts the machine down. Storage may still be billed; nothing is deleted.",
      when(controls.shutdown, button("Shut down", async () => { await run("stop"); await refreshAll(); }, "danger"), "No running worker.")],
    ["Emergency shutdown", "Use if the coordinator is unavailable. Can interrupt paid jobs; storage remains billable. The CLI still verifies the exact VM.",
      when(controls.exists, button("Emergency shutdown", async () => { await run("stop", { force: true }, { label: "Emergency shutdown" }); await refreshAll(); }, "danger"), "No deployment recorded.")],
    ["Delete worker", "Permanently deletes this VM and its storage. Billing for it stops; jobs in progress are lost. You can deploy a new worker afterwards.",
      when(controls.exists, button(["deleting", "delete_intent"].includes(ph) ? "Check deletion" : "Delete", async () => { await removeWorker(d.appId, true, null); }, "danger"), "No worker recorded on this computer.")],
    ["Redeploy", "Deletes this worker, waits until Phala removes it, then deploys a fresh one at up to the price below.",
      when(controls.exists && !controls.pending, button("Delete and deploy again", async () => { await removeWorker(d.appId, true, startLimit.value); }, "danger"), "No worker recorded on this computer.")],
    ["Start a stopped worker", "Starts the same machine again at up to this compute price.", h("div", { class: "row" }, startLimit, h("span", { class: "small muted" }, "USD/h max"),
      when(controls.start, button("Start", async () => { await run("start", { maxHourlyUsd: startLimit.value }); await refreshAll(); }), "Only a stopped worker without an unresolved start can be started."))],
  ];
  const renderActions = rows => h("ul", { class: "checks" }, rows.map(([t, desc, ctl]) => h("li", { class: "action-row", "data-action": t },
    h("span"), h("div", { class: "row" }, h("div", { class: "grow" }, h("div", { class: "name" }, t), h("div", { class: "detail" }, desc)), ctl))));
  const advanced = new Set(["Emergency shutdown", "Delete worker", "Redeploy"]);
  root.append(card("Actions", null, renderActions(actions.filter(([t]) => !advanced.has(t)))));
  // Stays open across refreshes, e.g. after cancelling an emergency shutdown.
  root.append(h("details", { class: "card", open: S.advancedOpen, ontoggle: e => { S.advancedOpen = e.target.open; } }, h("summary", null, "Advanced · emergency shutdown, deletion and redeploy"), renderActions(actions.filter(([t]) => advanced.has(t)))));
  const logs = card("Worker logs", [button("Read logs", async () => { await run("logs"); })],
    h("p", { class: "small muted" }, "Read bounded, sanitized worker events. Private logs may require the Phala console."));
  logs.id = "worker-logs"; root.append(logs);
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
  const q = h("input", { type: "text", placeholder: "Search errors", "aria-label": "Search errors" });
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
  Boot.begin("Checking your miner");
  // The version and UID already show in the card, so these two only get a row when they go wrong.
  const s = await api("/api/session");
  if (!s.ok) { Object.assign(Boot.step("session", "Connecting to everycli on this computer"), { short: "everycli" }).fail(s.data?.error?.message || "The local everycli UI did not answer."); Boot.finish(); return; }
  S.session = s.data;
  restorePendingOperations();
  await loadOverview();
  const prof = () => Object.assign(Boot.step("profile", "Loading your miner profile"), { short: "Miner profile" });
  if (!S.overview) prof().fail("Could not read the local miner profile.");
  else if (!S.overview.initialized) { const st = S.overview.steps || []; prof().warn(st.filter(stepDone).length + " of " + st.length + " setup steps done"); }
  readModelsRoute();
  const w = await api("/api/wallets"); if (w.ok) S.wallets = w.data;
  render();
  if (!managedHere() && S.overview?.initialized && (S.overview?.keys || []).some((k) => k.provider === "phala" && k.saved)) {
    await Boot.track(Boot.step("workloads", "Phala workers"), loadWorkloads, () => S.workloads?.length ? ["warn", S.workloads.length + " not managed here"] : ["ok", "none unmanaged"]);
    if (currentRoute() === "overview" && !S.renaming) render();
  } else if (!managedHere()) await loadWorkloads();
  if (S.overview?.initialized) {
    await bootSteps();
    if (["overview", "earnings"].includes(currentRoute()) && !S.renaming) render(); else { renderMeta(); renderNav(); patchSetupProgress(); patchWorkerView(); }
  }
  Boot.finish();
  if (!S.overview?.initialized && currentRoute() === "keys") loadBalances();
  if (S.overview?.initialized && currentRoute() === "worker") loadWorkerTools();
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
  readModelsRoute();
  S.openStep = null; S.renaming = false; render(); window.scrollTo(0, 0); $("#main").focus({ preventScroll: true });
  if (currentRoute() === "keys") loadBalances();
  if (currentRoute() === "worker") loadWorkerTools();
});
boot();

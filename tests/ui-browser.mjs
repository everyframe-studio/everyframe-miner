// Browser regression tests. Real loopback server/pairing, isolated profile,
// mocked external command results. Never uses wallet files or cloud mutations.
import assert from 'node:assert/strict';
import { mkdtemp, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { chromium } from 'playwright';

const scratch = await mkdtemp(join(await realpath(tmpdir()), 'everycli-ui-test-'));
const cli = spawn(resolve(process.env.EVERYCLI_BIN || 'target/debug/everycli'),
  ['ui', '--no-open', '--state-dir', join(scratch, 'profile')],
  { stdio: ['ignore', 'ignore', 'pipe'], env: { ...process.env, EVERYCLI_DISABLE_AUTO_UPDATE: '1' } });
let browser;
try {
  const url = await new Promise((resolveUrl, reject) => {
    let output = '';
    const timer = setTimeout(() => reject(Error('UI startup timeout')), 10000);
    cli.once('error', e => { clearTimeout(timer); reject(e); });
    cli.once('exit', c => { clearTimeout(timer); reject(Error(`UI exited ${c}`)); });
    cli.stderr.on('data', d => {
      output += d;
      const found = output.match(/http:\/\/127\.0\.0\.1:\d+\/#pair=[0-9a-f]+/);
      if (found) { clearTimeout(timer); resolveUrl(found[0]); }
    });
  });
  browser = await chromium.launch({ executablePath: process.env.CHROME_BIN || undefined, args: ['--no-sandbox'] });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.route('**/api/wallets', r => r.fulfill({ json: { wallets: [], root: '/fixture/wallets' } }));
  // The browser server is real, but no network-facing CLI command is executed.
  await page.route('**/api/run', r => r.fulfill({ status: 202, json: { job: 'fixture' } }));
  await page.route('**/api/jobs/fixture', r => r.fulfill({ json: { state: 'done', result: { value: { updateAvailable: false }, text: 'fixture' } } }));
  await page.addInitScript(() => {
    window.reviewIntervals = [];
    const original = window.setInterval;
    window.setInterval = (fn, ms, ...args) => { window.reviewIntervals.push({ fn, ms }); return original(fn, ms, ...args); };
  });
  await page.goto(url);
  await page.getByRole('heading', { name: 'Setup guide', exact: true }).waitFor();
  await page.waitForFunction(() => window.reviewIntervals.some(x => x.ms === 30000));
  const input = page.getByPlaceholder('/path/to/hotkey');
  await input.fill('/fixture/miner/hotkeys/default');
  await page.evaluate(async () => { await window.reviewIntervals.find(x => x.ms === 30000).fn(); });
  assert.equal(await input.inputValue(), '/fixture/miner/hotkeys/default', 'background polling must preserve setup draft');
  assert.equal(await page.evaluate(() => location.hash), '', 'covers the default route, not only #/setup');
  await page.reload();
  await page.getByRole('heading', { name: 'Setup guide', exact: true }).waitFor();
  assert.equal(await page.locator('h1').textContent(), 'Setup guide', 'paired tab survives refresh');

  // Drive browser rendering with the exact native balance/health schemas.
  await page.evaluate(() => {
    S.overview = { initialized: true, label: 'studio-worker-1', deployment: { phase: 'running', appId: '1'.repeat(40) }, profile: {}, keys: [], steps: ['hotkey','phala','providers','init','deploy','activate','offers','resume'].map(id => ({ id, done: true })) };
    S.health = { result: { value: { ok: false, checks: [{ name: 'Current model bid', ok: true }, { name: 'Accepting new work', ok: false }] } } };
    history.replaceState(null, '', '#/setup'); render();
  });
  assert.equal(await page.getByText('Setup complete', { exact: true }).count(), 0, 'paused worker is not ready');
  await page.evaluate(() => { S.health.result.value.ok = true; S.health.result.value.checks[1].ok = true; render(); });
  assert.equal(await page.getByText('Setup complete', { exact: true }).count(), 1);
  await page.evaluate(() => {
    S.health = { state: 'failed', result: { error: { message: 'Offline' } } }; render();
  });
  assert.equal(await page.getByText('Setup complete', { exact: true }).count(), 0, 'failed health cannot retain readiness');

  for (const phase of ['stopping', 'starting', 'provisioned', 'updating']) {
    await page.evaluate(phase => { S.overview.deployment.phase = phase; S.overview.deployment.needsReconcile = true; history.replaceState(null, '', '#/worker'); render(); }, phase);
    assert.equal(await page.getByRole('button', { name: 'Re-sync operation', exact: true }).isEnabled(), true, phase);
    assert.equal(await page.getByRole('button', { name: 'Re-sync state', exact: true }).isEnabled(), true, phase);
  }
  await page.evaluate(() => { S.overview.deployment.phase = 'stopped'; S.overview.deployment.needsReconcile = false; render(); });
  assert.equal(await page.getByRole('button', { name: 'Start', exact: true }).isEnabled(), true);
  await page.evaluate(() => { S.overview.deployment.phase = 'awaiting_readmission'; render(); });
  assert.equal(await page.getByRole('button', { name: 'Resume', exact: true }).isEnabled(), true);
  // Worker drafts survive background updates, including after the input blurs.
  await page.getByRole('button', { name: 'Rename', exact: true }).click();
  await page.getByRole('textbox', { name: 'Worker label' }).fill('unsaved-label');
  await page.getByRole('heading', { name: 'Worker', exact: true }).click();
  await page.evaluate(async () => { await window.reviewIntervals.find(x => x.ms === 30000).fn(); });
  assert.equal(await page.getByRole('textbox', { name: 'Worker label' }).inputValue(), 'unsaved-label');
  assert.equal(await page.getByText('Renew with everycli init', { exact: true }).count(), 0);
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();

  await page.evaluate(() => {
    S.overview.initialized = true;
    S.balances = { result: { value: { balances: [
      { provider: 'vivix', status: 'ok', remainingUsd: null, nativeBalance: { unit: 'W_CREDITS', remainingMicrounits: 130000000 } },
      { provider: 'fal', status: 'unavailable', diagnostic: { message: 'Billing permission required', next: 'Add a billing key' } }
    ] } } };
    history.replaceState(null, '', '#/overview'); render();
  });
  assert.match(await page.locator('tr').filter({ hasText: 'Vivix' }).innerText(), /130 W credits/);
  assert.equal(await page.getByText('Billing permission required', { exact: true }).count(), 1);
  assert.equal(await page.evaluate(() => balanceAmount({ provider: 'vivix', nativeBalance: { unit: 'W_CREDITS', remainingMicrounits: 0 } })), '0 W credits');
  assert.equal(await page.evaluate(() => balanceAmount({ provider: 'vivix', status: 'unavailable' })), '—');

  await page.evaluate(() => {
    S.overview.initialized = true;
    S.overview.network = 'mainnet';
    S.overview.profile = { hotkey: '5FW5r4Hjpa7m2hsU5h3awcAwmErcfurCyFqB3Fi1fv8quSvT' };
    S.overview.registration = { registered: true, network: 'mainnet', hotkey: S.overview.profile.hotkey, uid: 0, checkedAt: Date.now() };
    render();
  });
  const identity = page.getByLabel('Miner identity', { exact: true });
  assert.match(await identity.innerText(), /UID 0/);
  assert.match(await identity.innerText(), /5FW5r4…8quSvT/);
  assert.equal(await identity.locator('code').getAttribute('title'), '5FW5r4Hjpa7m2hsU5h3awcAwmErcfurCyFqB3Fi1fv8quSvT');
  await page.evaluate(() => { S.overview.registration.hotkey = 'wrong'; render(); });
  assert.match(await identity.innerText(), /UID not checked/);
  await page.evaluate(() => { S.overview.registration.hotkey = S.overview.profile.hotkey; S.overview.registration.uid = 127; S.overview.recoveryPending = true; render(); });
  assert.match(await identity.innerText(), /UID 127/);
  assert.equal(await page.getByRole('button', { name: 'Reconnect profile', exact: true }).count(), 1);
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await identity.evaluate(el => el.scrollWidth <= el.clientWidth), true);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.evaluate(() => { S.overview.recoveryPending = false; render(); });

  await page.evaluate(() => { window.confirmResult = null; confirmDialog('Test question only', 'Test').then(v => window.confirmResult = v); });
  assert.equal(await page.locator('#app').evaluate(e => e.inert), true);
  for (let i = 0; i < 6; i++) await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(() => !!document.activeElement.closest('#modal')), true);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => window.confirmResult), false);
  assert.equal(await page.locator('#app').evaluate(e => e.inert), false);

  // Failed deletion checks must never become a successful replacement.
  const deletion = await page.evaluate(async () => {
    const originals = { run, loadOverview, loadWorkloads, refreshAll, timeout: window.setTimeout };
    const calls = [];
    try {
      run = async (command) => {
        calls.push(command);
        return calls.length === 1 ? { state: 'done', result: { value: { phase: 'deleting' } } }
          : { state: 'failed', result: { error: { error: 'request_timeout' } } };
      };
      loadOverview = loadWorkloads = refreshAll = async () => {};
      window.setTimeout = (fn) => originals.timeout.call(window, fn, 0);
      await removeWorker('a'.repeat(40), false, '0.06');
      return { calls, pending: S.removal, name: shortName('everyframe-worker-ec7d96'), missingCheck: checkMessage('fal', null, true).kind };
    } finally {
      ({ run, loadOverview, loadWorkloads, refreshAll } = originals);
      window.setTimeout = originals.timeout;
    }
  });
  assert.deepEqual(deletion.calls, ['delete', 'delete']);
  assert.equal(deletion.pending, null);
  assert.equal(deletion.name, 'every…ec7d96');
  assert.equal(deletion.missingCheck, 'warn');

  // Reopened setup shows the initialized identity even without in-memory choice.
  await page.evaluate(() => {
    S.hotkeySel = null; S.wallets = { wallets: [] };
    S.overview = { initialized: true, hotkeyAttached: true, network: 'mainnet', profile: { hotkey: '5FW5r4Hjpa7m2hsU5h3awcAwmErcfurCyFqB3Fi1fv8quSvT' }, deployment: { phase: 'deleted' }, keys: [], steps: [{ id: 'hotkey', done: true }, { id: 'deploy', done: false }] };
    S.openStep = 'hotkey'; history.replaceState(null, '', '#/setup'); render();
  });
  assert.equal(await page.getByText('Initialized hotkey', { exact: true }).count(), 1);
  assert.match(await page.locator('#main').innerText(), /Attached on this computer/);
  await page.evaluate(() => { S.openStep = 'deploy'; S.workloads = []; render(); });
  assert.equal(await page.getByLabel('Maximum compute USD per hour', { exact: true }).inputValue(), '10');

  await page.evaluate(() => {
    S.offers = { offers: [
      { model: 'fixture/a', name: 'Alpha', offered: true, offerable: true, discountBp: 0, pricingCurrent: true, baseMinerRewardMicrousd: 1000000 },
      { model: 'fixture/b', name: 'Beta', offered: false, offerable: true, localKeyConfigured: true, baseMinerRewardMicrousd: 2000000 },
      { model: 'fixture/c', name: 'Blocked', offered: false, offerable: false, blockers: ['integration_required'] },
    ] };
    S.offersError = null; S.offerFilter = 'all'; S.selectedOffers.clear(); S.batchReport = null;
    history.replaceState(null, '', '#/models'); render();
  });
  await page.getByLabel('Select all editable models in this tab').check();
  assert.equal(await page.getByLabel('Select Alpha', { exact: true }).isChecked(), true);
  assert.equal(await page.getByLabel('Select Beta', { exact: true }).isChecked(), true);
  assert.equal(await page.getByLabel('Select Blocked', { exact: true }).isDisabled(), true);
  await page.getByRole('button', { name: 'My active offers', exact: true }).click();
  assert.equal(await page.getByLabel('Select Alpha', { exact: true }).isChecked(), false, 'changing tabs clears hidden selection');
  assert.equal(await page.getByLabel('Select Beta', { exact: true }).count(), 0);
  await page.getByRole('button', { name: 'Ready for my keys', exact: true }).click();
  assert.equal(await page.getByLabel('Select Beta', { exact: true }).count(), 1);

  const batch = await page.evaluate(async () => {
    const originals = { run, confirmDialog, loadOffers };
    const calls = [], questions = [];
    try {
      loadOffers = async () => {};
      confirmDialog = async question => { questions.push(question.textContent); return true; };
      run = async (action, params, opts) => {
        calls.push({ action, params, opts });
        if (opts.prepareOnly) return { state: 'confirm', result: { question: `Offer ${params.model} at 10% discount`, hash: `quote-${params.model}` } };
        if (params.model === 'fixture/b') return { state: 'confirm', result: { question: 'Changed price', hash: 'changed' } };
        return { state: 'done', result: { value: {} } };
      };
      S.selectedOffers = new Set(['fixture/a', 'fixture/b']); S.batchDiscount = '10';
      await batchOffers(S.offers.offers, false);
      const result = { calls: [...calls], questions: [...questions], report: [...S.batchReport], selected: [...S.selectedOffers] };
      // Cancellation approves none of the prepared hashes.
      calls.length = 0; confirmDialog = async () => false;
      await batchOffers(S.offers.offers, false);
      result.cancelCalls = [...calls];
      // Withdraw active selections only; failures remain visible and selected.
      calls.length = 0; confirmDialog = async () => true;
      run = async (action, params, opts) => {
        calls.push({ action, params, opts });
        return opts.prepareOnly ? { state: 'confirm', result: { question: 'Withdraw this offer?', hash: 'withdraw-hash' } }
          : { state: 'failed', result: { error: { message: 'Coordinator unavailable; check offers before retrying.' } } };
      };
      S.selectedOffers = new Set(['fixture/a', 'fixture/b']);
      await batchOffers(S.offers.offers, true);
      result.withdrawCalls = [...calls]; result.withdrawReport = [...S.batchReport];
      return result;
    } finally { ({ run, confirmDialog, loadOffers } = originals); }
  });
  assert.equal(batch.questions.length, 1, 'one review for the batch');
  assert.match(batch.questions[0], /fixture\/a/);
  assert.match(batch.questions[0], /fixture\/b/);
  assert.equal(batch.calls.length, 4);
  assert.deepEqual(batch.calls[2].opts.approvals, ['quote-fixture/a']);
  assert.equal(batch.calls[2].opts.exactApproval, true);
  assert.equal(batch.report[0].ok, true);
  assert.match(batch.report[1].message, /Quote changed/);
  assert.deepEqual(batch.selected, ['fixture/b']);
  assert.equal(batch.cancelCalls.every(c => c.opts.prepareOnly), true, 'cancel never submits an offer');
  assert.equal(batch.withdrawCalls.length, 2);
  assert.equal(batch.withdrawCalls.every(c => c.params.model === 'fixture/a' && c.params.withdraw === true), true);
  assert.equal(batch.withdrawReport.some(r => !r.ok && /Coordinator unavailable/.test(r.message)), true);

  // run itself must not approve a changed quote or open another modal in a batch.
  const approvalGuard = await page.evaluate(async () => {
    const originals = { api, poll, confirmDialog };
    let dialogs = 0, submitted;
    try {
      api = async (path, body) => { submitted = body; return { ok: true, data: { job: 'fixture' } }; };
      poll = async () => ({ state: 'confirm', result: { hash: 'new-quote', question: 'new price' } });
      confirmDialog = async () => { dialogs++; return true; };
      const job = await run('offer', { model: 'fixture/b', discountPct: '10' }, { quiet: true, approvals: ['reviewed-quote'], exactApproval: true });
      return { state: job.state, submitted, dialogs };
    } finally { ({ api, poll, confirmDialog } = originals); }
  });
  assert.equal(approvalGuard.state, 'confirm');
  assert.equal(approvalGuard.dialogs, 0);
  assert.deepEqual(approvalGuard.submitted.approvals, ['reviewed-quote']);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => { S.offerFilter = 'all'; render(); });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'batch models fit mobile');
  await page.evaluate(() => { history.replaceState(null, '', '#/setup'); render(); });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'mobile viewport must not overflow');
  assert.deepEqual(errors, []);
  console.log('UI browser regressions passed: pairing, drafts, lifecycle, readiness, W credits, diagnostics, modal and mobile.');
} finally {
  if (browser) await browser.close();
  if (cli.exitCode === null) { const stopped = once(cli, 'exit'); cli.kill('SIGTERM'); await stopped; }
  await rm(scratch, { recursive: true, force: true });
}

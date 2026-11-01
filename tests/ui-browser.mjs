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
  assert.equal(await page.getByRole('heading', { name: 'Select your hotkey', exact: true }).count(), 1);
  assert.equal(await page.getByRole('button', { name: 'Check registration', exact: true }).count(), 1);
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
    S.health.result.value.ok=false;
    S.health.result.value.checks.push({name:'Model: expired-offer',ok:false,detail:'Offer pricing needs attention.'});
    render();
  });
  assert.equal(await page.locator('[data-setup-progress]').innerText(),'8 of 8 steps done','one model warning cannot undo confirmed routing');
  assert.equal(await page.getByText('Setup complete',{exact:true}).count(),1);
  assert.match(await page.locator('#setup-complete').innerText(),/Some health checks still need attention/);
  await page.evaluate(() => {
    S.health = { state: 'failed', result: { error: { message: 'Offline' } } }; render();
  });
  assert.equal(await page.getByText('Setup complete', { exact: true }).count(), 0, 'failed health cannot retain readiness');

  // Direct Kling is a separate key, not a re-labeled Fal credential.
  await page.evaluate(() => {
    S.overview.keys = [{ provider: 'kling', kind: 'generation', saved: false }];
    history.replaceState(null, '', '#/keys'); render();
  });
  assert.equal(await page.getByText('Kling (direct)', { exact: true }).count(), 1);
  assert.equal(await page.getByText(/Kling developer API key, not a Fal key/).count(), 1);
  assert.equal(await page.locator('input[type="password"]').count(), 1);
  await page.evaluate(() => { S.overview.keys = []; });

  for (const phase of ['stopping', 'starting', 'provisioned', 'updating']) {
    await page.evaluate(phase => { S.overview.deployment.phase = phase; S.overview.deployment.needsReconcile = true; history.replaceState(null, '', '#/worker'); render(); }, phase);
    assert.equal(await page.getByRole('button', { name: 'Re-sync operation', exact: true }).isEnabled(), true, phase);
    assert.equal(await page.getByRole('button', { name: 'Re-sync state', exact: true }).isEnabled(), true, phase);
  }
  await page.evaluate(()=>{S.overview.deployment.phase='running';S.overview.deployment.needsReconcile=false;S.health={result:{value:{cloudStatus:'stopped',checks:[]}}};render();});
  assert.equal(await page.getByRole('button',{name:'Start',exact:true}).isEnabled(),true);
  assert.equal(await page.getByRole('button',{name:'Resume',exact:true}).isEnabled(),false);
  for (const phase of ['start_intent','starting']) {
    await page.evaluate(phase=>{S.overview.deployment.phase=phase;render();},phase);
    assert.equal(await page.getByRole('button',{name:'Start',exact:true}).isEnabled(),false,phase);
  }
  await page.evaluate(()=>{S.health=null;});
  await page.evaluate(() => { S.overview.deployment.phase = 'stopped'; S.overview.deployment.needsReconcile = false; render(); });
  assert.equal(await page.getByRole('button', { name: 'Start', exact: true }).isEnabled(), true);
  await page.evaluate(() => { S.overview.deployment.phase = 'awaiting_readmission'; render(); });
  assert.equal(await page.getByRole('button', { name: 'Resume', exact: true }).isEnabled(), false, 'resume waits for verified readiness');
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
    window.balanceTestOriginalRun=run;
    S.overview.keys=[{provider:'fal',kind:'generation',saved:true}];
    history.replaceState(null,'','#/keys');render();
    run=async(command,...args)=>command==='balances'?{result:{value:{balances:[{provider:'fal',status:'ok',remainingUsd:42.5,checkedAt:Date.now()}]}}}:window.balanceTestOriginalRun(command,...args);
  });
  assert.equal(await page.getByRole('heading',{name:'Provider balances',exact:true}).count(),1);
  const keyDraft=page.locator('input[type="password"]').first();
  await keyDraft.fill('unsaved-provider-key-draft');
  await page.locator('#provider-balances').getByRole('button',{name:'Refresh',exact:true}).click();
  assert.equal(await keyDraft.inputValue(),'unsaved-provider-key-draft','refreshing balances must preserve API-key input');
  assert.match(await page.locator('#provider-balances').innerText(),/42\.50/);
  await page.evaluate(()=>{run=window.balanceTestOriginalRun;delete window.balanceTestOriginalRun;S.overview.keys=[];history.replaceState(null,'','#/overview');render();});

  const summary = page.getByLabel('Miner overview summary', { exact: true });
  assert.deepEqual(await summary.locator('.label').allTextContents(), ['Worker', 'Status', 'Active offers', 'Jobs']);
  assert.equal(await summary.locator('.stat').count(), 4);
  assert.equal(await summary.evaluate(el => getComputedStyle(el).gridTemplateColumns.split(' ').length), 4);
  const statusCases = [
    { health: null, expected: 'Checking…' },
    { health: { state: 'failed', result: { error: { message: 'Offline' } } }, expected: 'Unknown' },
    { health: { result: { value: { ok: true, checks: [] } } }, expected: 'Unknown' },
    { health: { result: { value: { ok: false, checks: [{ name: 'Accepting new work', ok: false }] } } }, expected: 'Paused' },
    { health: { result: { value: { ok: false, checks: [{ name: 'Accepting new work', ok: true }, { name: 'Workload attestation', ok: false }] } } }, expected: 'Needs attention' },
    { health: { result: { value: { ok: true, checks: [{ name: 'Accepting new work', ok: true }], activity: { activeJobs: 0 } } } }, expected: 'Ready' },
    { health: { result: { value: { ok: true, checks: [{ name: 'Accepting new work', ok: true }], activity: { activeJobs: 1 } } } }, expected: 'Working' },
  ];
  for (const test of statusCases) {
    await page.evaluate(health => { S.health = health; render(); }, test.health);
    assert.equal(await summary.locator('.stat').nth(1).locator('.value').textContent(), test.expected);
  }
  for (const [width, columns] of [[1000, 2], [390, 1], [1440, 4]]) {
    await page.setViewportSize({ width, height: 1000 });
    assert.equal(await summary.evaluate(el => getComputedStyle(el).gridTemplateColumns.split(' ').length), columns);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'overview must fit viewport');
  }
  await page.setViewportSize({width:390,height:844});
  await page.evaluate(()=>{window.phaseBeforeHistoryTest=S.overview.deployment.phase;S.overview.deployment.phase='awaiting_readmission';render();});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'long worker-status pills must fit mobile');
  await page.evaluate(()=>{S.overview.deployment.phase=window.phaseBeforeHistoryTest;render();});
  await page.setViewportSize({width:1440,height:1000});

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
  assert.equal(await page.getByRole('heading', { name: 'Your hotkey', exact: true }).count(), 1);
  assert.equal(await page.getByText('Attached hotkey', { exact: true }).count(), 1);
  assert.match(await page.locator('#main').innerText(), /Attached on this computer/);
  await page.evaluate(() => {
    S.overview.registration = { hotkey: S.overview.profile.hotkey, registered: true, network: 'mainnet', uid: 0 }; render();
  });
  assert.equal(await page.getByText('UID 0', { exact: true }).count(), 1);
  await page.evaluate(() => { S.overview.registration.hotkey = 'different'; render(); });
  assert.equal(await page.getByText('UID not checked', { exact: true }).count(), 1);
  assert.equal(await page.getByRole('button', { name: 'Check registration', exact: true }).count(), 1);
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
  assert.equal(await page.getByLabel('Select Blocked', { exact: true }).count(), 0, 'unintegrated models are hidden');
  assert.equal(await page.getByText('Integration pending', { exact: true }).count(), 0);
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
      confirmDialog = async question => { questions.push(question); return true; };
      run = async (action, params, opts) => {
        calls.push({ action, params, opts });
        if (opts.prepareOnly) return { state: 'confirm', result: { question: `Offer ${params.models} at 10% discount`, hash: 'reviewed-batch' } };
        return { state: 'done', result: { value: {} } };
      };
      S.selectedOffers = new Set(['fixture/a', 'fixture/b']); S.batchDiscount = '10';
      await batchOffers(S.offers.offers, false);
      const result = { calls: [...calls], questions: [...questions], report: [...S.batchReport], selected: [...S.selectedOffers] };
      // Cancellation approves none of the prepared hashes.
      calls.length = 0; confirmDialog = async () => false; S.selectedOffers = new Set(['fixture/a','fixture/b']);
      await batchOffers(S.offers.offers, false);
      result.cancelCalls = [...calls];
      calls.length = 0; confirmDialog = async () => true;
      run = async (action, params, opts) => {
        calls.push({action,params,opts});
        return { state: 'confirm', result: { question: 'Changed quote', hash: opts.prepareOnly ? 'before' : 'after' } };
      };
      await batchOffers(S.offers.offers,false); result.changedReport=[...S.batchReport]; result.changedCalls=[...calls];
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
  assert.equal(batch.calls.length, 2, 'one preparation and one commit, independent of model count');
  assert.equal(batch.calls.every(c=>c.action==='offer-batch'),true);
  assert.deepEqual(batch.calls[1].opts.approvals, ['reviewed-batch']);
  assert.equal(batch.calls[1].opts.exactApproval, true);
  assert.equal(batch.report[0].ok, true);
  assert.equal(batch.report[1].ok, true);
  assert.deepEqual(batch.selected, []);
  assert.equal(batch.changedReport.every(r=>!r.ok && /Quote changed/.test(r.message)),true);
  assert.equal(batch.changedCalls.length,2,'changed quotes are not replayed');
  assert.equal(batch.cancelCalls.every(c => c.opts.prepareOnly), true, 'cancel never submits an offer');
  assert.equal(batch.withdrawCalls.length, 2);
  assert.equal(batch.withdrawCalls.every(c => c.params.models === 'fixture/a' && c.params.withdraw === true), true);
  assert.equal(batch.withdrawReport.some(r => !r.ok && /Coordinator unavailable/.test(r.message)), true);

  // run itself must not approve a changed quote or open another modal in a batch.
  const availabilityUI = await page.evaluate(async () => {
    const original = S.offers;
    try {
      S.offers = {offers:[
        {model:'studio/openrouter/example',providerApiId:'vendor/example',name:'Supported fixture',provider:'openrouter',offerable:true,catalogGroup:'supported'},
        {model:'retired/example',providerApiId:'vendor/old',name:'Retired fixture',offered:true,offerable:false,catalogGroup:'candidate',providerAvailability:{status:'deprecated'}},
        {model:'candidate/example',name:'Candidate fixture',offerable:false,catalogGroup:'candidate'},
      ]};
      S.offerFilter='all';render();document.querySelectorAll('.model-details').forEach(d=>d.open=true);const all=document.querySelector('#main').innerText;
      S.offerFilter='candidate';render();const candidates=document.querySelector('#main').innerText;
      const canUpdate=[...document.querySelectorAll('button')].some(b=>b.textContent==='Update'&&!b.disabled);
      const canWithdraw=[...document.querySelectorAll('button')].some(b=>b.textContent==='Withdraw'&&!b.disabled);
      S.selectedOffers=new Set(['retired/example']);await batchOffers(S.offers.offers,false);
      return {all,candidates,canUpdate,canWithdraw,report:S.batchReport};
    } finally {S.offers=original;S.offerFilter='all';S.selectedOffers.clear();clearBatchReport();render();}
  });
  assert.match(availabilityUI.all,/Everyframe contract: studio\/openrouter\/example/);
  assert.match(availabilityUI.all,/Provider API ID: vendor\/example/);
  assert.doesNotMatch(availabilityUI.all,/Retired fixture|Candidate fixture/);
  assert.match(availabilityUI.candidates,/Deprecated by provider/);
  assert.equal(availabilityUI.canUpdate,false);
  assert.equal(availabilityUI.canWithdraw,true);
  assert.match(availabilityUI.report[0].message,/Skipped.*deprecated/);

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

  const badges = await page.evaluate(() => {
    S.overview.profile.minerId='badge-fixture';const key=modelSeenKey();localStorage.removeItem(key);S.modelSeen.delete(key);
    trackModels([{model:'fixture/a',contractAvailable:true}]);const baseline=S.newModels.size;
    const rows=[{model:'fixture/a',contractAvailable:true},{model:'fixture/new',contractAvailable:true},{model:'pending',contractAvailable:false,blockers:['integration_required']}];
    trackModels(rows);const added=[...S.newModels];S.offers={offers:rows};render();
    const badge=document.querySelector('#main .seg').textContent;
    S.modelSeen.clear();trackModels(rows);const persisted=[...S.newModels];
    acknowledgeModels();trackModels(rows);const acknowledged=S.newModels.size;
    return {baseline,added,badge,persisted,acknowledged};
  });
  assert.equal(badges.baseline,0);assert.deepEqual(badges.added,['fixture/new']);
  assert.match(badges.badge,/1 new/);assert.deepEqual(badges.persisted,['fixture/new']);assert.equal(badges.acknowledged,0);

  const expiry = await page.evaluate(() => {
    const original=window.setTimeout;let callback,delay;
    try {
      window.setTimeout=(fn,ms)=>{callback=fn;delay=ms;return 12345;};
      S.offerFilter='all';
      showBatchReport([{model:'fixture/a',ok:true,message:'Saved'}]);render();
      const draft=document.querySelector('input[aria-label="Discount percent for fixture/a"]');draft.value='23';
      const visible=!!S.batchReport,successDelay=delay;callback();
      const cleared=S.batchReport===null;
      const draftPreserved=document.querySelector('input[aria-label="Discount percent for fixture/a"]')===draft && draft.value==='23';
      const reportRemoved=document.querySelector('[data-batch-report]')===null;
      showBatchReport([{model:'fixture/a',ok:false,message:'Failed'}]);const failureDelay=delay;
      return {visible,cleared,successDelay,failureDelay,draftPreserved,reportRemoved};
    } finally {window.setTimeout=original;clearBatchReport();}
  });
  assert.equal(expiry.visible,true);assert.equal(expiry.cleared,true);assert.equal(expiry.successDelay,12000);assert.equal(expiry.failureDelay,30000);
  assert.equal(expiry.draftPreserved,true);assert.equal(expiry.reportRemoved,true);
  const navigated = await page.evaluate(async () => {
    const originals={run,loadOffers,confirmDialog};let calls=0;
    try {
      S.offers={offers:[{model:'fixture/a',offerable:true}]};S.selectedOffers=new Set(['fixture/a']);
      loadOffers=async()=>{};confirmDialog=async()=>true;
      run=async()=>{calls++;S.navigationEpoch++;history.replaceState(null,'','#/worker');return {state:'confirm',result:{question:'review',hash:'hash'}};};
      await batchOffers(S.offers.offers,false);
      return {calls,report:S.batchReport};
    } finally {({run,loadOffers,confirmDialog}=originals);history.replaceState(null,'','#/models');}
  });
  assert.equal(navigated.calls,1,'leaving during preparation never submits');assert.equal(navigated.report,null,'late result does not reappear after navigation');
  await page.evaluate(()=>{showBatchReport([{model:'fixture/a',ok:true,message:'Saved'}]);go('overview');});
  await page.waitForFunction(()=>currentRoute()==='overview'&&S.batchReport===null);
  await page.evaluate(()=>{
    S.health={state:'done',result:{value:{ok:true,checks:[],activity:{jobsByModel:[{model:'fixture/video',completed:32,active:1,failed:2,cancelled:0}]}}}};
    S.earnings={state:'done',result:{value:{currency:'USD',unit:'microusd',summary:{totalWorkValueMicrousd:1234567,pendingReviewMicrousd:0,approvedMicrousd:1234567,markedRewardEligibleMicrousd:1234567,excludedMicrousd:0},fees:[{state:'approved',jobs:32,amountMicrousd:1234567}],payoutsImplemented:false,paidMicrousd:null}}};render();
  });
  assert.equal(await page.getByRole('heading',{name:'Jobs by model',exact:true}).count(),1);
  assert.match(await page.locator('tr').filter({hasText:'fixture/video'}).innerText(),/32/);
  assert.match(await page.locator('#main').innerText(),/\$1\.234567/);
  assert.doesNotMatch(await page.locator('#main').innerText(),/amountMicrousd|payoutsImplemented|\[\{/);

  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'job and earnings tables fit mobile');
  await page.evaluate(() => { history.replaceState(null,'','#/models');S.offerFilter = 'all'; render(); });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'batch models fit mobile');
  await page.evaluate(() => { history.replaceState(null, '', '#/setup'); render(); });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'mobile viewport must not overflow');

  // Individual history uses the same read-only command, with cursor-bound filters.
  await page.evaluate(() => {
    window.historyRunOriginal=run;window.historyCalls=[];document.querySelector('#activity').replaceChildren();
    const completed={jobId:'job-accepted-1',attemptId:'attempt-accepted-1',name:'H3 Max Turbo',model:'minimax/h3-max-turbo/text-to-video',provider:'fal',kind:'video',durationSeconds:5,resolution:'768p',state:'accepted',jobCreatedAt:Date.now()-120000,startedAt:Date.now()-90000,completedAt:Date.now()-30000,reviewState:'review',verification:'accepted',rewardEligibility:'pending_review',recordedWorkValueMicrousd:200000,providerCostEstimateMicrousd:250000};
    const failed={...completed,jobId:'job-failed-2',attemptId:'attempt-failed-2',name:'<img src=x onerror=alert(1)>',model:'direct/vivix/vivix-w1',provider:'vivix',state:'failed',completedAt:null,reviewState:'not_applicable',verification:'not_verified',rewardEligibility:'not_applicable',recordedWorkValueMicrousd:null,failureReason:'provider_billing_required'};
    window.historyFixture={completed,failed};
    run=async(action,params)=>{
      if(action!=='jobs')return window.historyRunOriginal(action,params,{quiet:true});
      window.historyCalls.push({...params});
      const rows=params.status==='failed'?[failed]:params.cursor?[{...completed,jobId:'older-job',attemptId:'older-attempt'}]:[completed,failed];
      return {state:'done',result:{value:{minerId:'fixture',asOf:Date.now(),historyAvailable:true,history:{version:1,currency:'USD',unit:'microusd',limit:20,rows,nextCursor:params.cursor||params.status==='failed'?null:'next_page',filters:{status:params.status||'all',model:params.model||null}}}}};
    };
    S.history=null;S.historyBusy=false;S.historyStatus='all';S.historyModel='';S.historyModelDraft='';S.historyCursors=[null];S.historyPage=0;
    history.replaceState(null,'','#/jobs');render();
  });
  await page.getByRole('button',{name:'View attempt attempt-failed-2',exact:true}).waitFor();
  assert.equal(await page.locator('#main img').count(),0,'model text cannot become HTML');
  await page.getByRole('button',{name:'View attempt attempt-failed-2',exact:true}).click();
  assert.match(await page.locator('#job-detail').innerText(),/Provider billing requires attention/);
  assert.match(await page.locator('#job-detail').innerText(),/Not recorded/);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'mobile attempt details fit');
  if(process.env.EVERYCLI_QA_DIR){await page.evaluate(()=>scrollTo(0,0));await page.screenshot({path:join(process.env.EVERYCLI_QA_DIR,'jobs-mobile.png'),fullPage:true});}
  await page.getByRole('button',{name:'Close details',exact:true}).click();
  await page.getByRole('button',{name:'Next page',exact:true}).click();
  await page.getByRole('button',{name:'View attempt older-attempt',exact:true}).waitFor();
  assert.equal(await page.getByRole('button',{name:'Next page',exact:true}).isDisabled(),true);
  assert.equal(await page.evaluate(()=>window.historyCalls.at(-1).cursor),'next_page');
  await page.getByRole('button',{name:'Previous page',exact:true}).click();
  await page.getByRole('button',{name:'View attempt attempt-accepted-1',exact:true}).waitFor();
  await page.getByLabel('Filter job status',{exact:true}).selectOption('failed');
  await page.waitForFunction(()=>!S.historyBusy&&S.history.result.value.history.rows.length===1);
  await page.getByLabel('Filter job model',{exact:true}).fill('direct/vivix/vivix-w1');
  await page.getByRole('button',{name:'Apply model filter',exact:true}).click();
  await page.waitForFunction(()=>!S.historyBusy&&window.historyCalls.at(-1).model==='direct/vivix/vivix-w1');
  assert.equal(await page.evaluate(()=>window.historyCalls.at(-1).status),'failed');
  assert.equal(await page.evaluate(()=>window.historyCalls.at(-1).cursor),undefined,'changed filters start at page one');
  await page.setViewportSize({width:1440,height:1000});
  if(process.env.EVERYCLI_QA_DIR)await page.screenshot({path:join(process.env.EVERYCLI_QA_DIR,'jobs-desktop.png'),fullPage:true});
  const race=await page.evaluate(async()=>{
    const original=run,pending=[];
    try{
      run=()=>new Promise(resolve=>pending.push(resolve));
      S.historyStatus='all';const older=loadHistory(true);
      S.historyStatus='failed';const newer=loadHistory(true);
      const answer=marker=>({state:'done',result:{value:{historyAvailable:true,history:{rows:[],nextCursor:null,marker}}}});
      pending[1](answer('new'));await newer;pending[0](answer('old'));await older;
      return S.history.result.value.history.marker;
    }finally{run=original;}
  });
  assert.equal(race,'new','late history responses cannot overwrite changed filters');
  await page.evaluate(()=>{S.history={state:'failed',result:{error:{message:'Coordinator unavailable'}}};render();});
  assert.equal(await page.getByText('Could not load job history',{exact:true}).count(),1);
  assert.equal(await page.locator('table.job-history').count(),0,'an error must not show stale history as current');
  await page.evaluate(()=>{S.history={state:'done',result:{value:{historyAvailable:true,history:{rows:[],nextCursor:null}}}};render();});
  assert.equal(await page.getByText('No attempts match these filters.',{exact:true}).count(),1);
  await page.evaluate(()=>{run=window.historyRunOriginal;S.history={state:'done',result:{value:{historyAvailable:false,note:'Coordinator upgrade required'}}};render();});
  assert.equal(await page.getByText('History is not available yet',{exact:true}).count(),1);

  await page.evaluate(()=>{
    S.earnings={state:'done',result:{value:{currency:'USD',unit:'microusd',asOf:Date.now(),summary:{totalWorkValueMicrousd:400000,pendingReviewMicrousd:200000,approvedMicrousd:200000,markedRewardEligibleMicrousd:100000,excludedMicrousd:0},fees:[{state:'review',jobs:1,amountMicrousd:200000},{state:'approved',jobs:1,amountMicrousd:200000}],payoutsImplemented:false,estimates:{providerExposureMicrousd:350000,quotedAttempts:2,unpricedAttempts:1,uncertainAttempts:1}}}};
    history.replaceState(null,'','#/earnings');render();
  });
  assert.equal(await page.getByRole('heading',{name:'Earnings',exact:true}).count(),1);
  assert.match(await page.locator('#main').innerText(),/\$0\.40/);
  assert.match(await page.locator('#main').innerText(),/Subset of approved work; do not add twice/);
  assert.match(await page.locator('#main').innerText(),/Net profit\s+Unavailable/);
  assert.doesNotMatch(await page.locator('#main').innerText(),/\$0\.050000/,'no invented profit from estimates');
  if(process.env.EVERYCLI_QA_DIR)await page.screenshot({path:join(process.env.EVERYCLI_QA_DIR,'earnings-desktop.png'),fullPage:true});
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'earnings report fits mobile');
  if(process.env.EVERYCLI_QA_DIR)await page.screenshot({path:join(process.env.EVERYCLI_QA_DIR,'earnings-mobile.png'),fullPage:true});
  await page.evaluate(()=>{S.earnings={state:'done',result:{value:{fees:[],payoutsImplemented:false}}};render();});
  assert.match(await page.locator('#main').innerText(),/Unknown amounts are not treated as zero/);
  assert.doesNotMatch(await page.locator('#main').innerText(),/\$0\.000000/);
  await page.evaluate(()=>{S.health={state:'failed',result:{error:{message:'Unavailable'}}};history.replaceState(null,'','#/overview');render();});
  assert.equal(await page.getByLabel('Miner overview summary').locator('.stat').nth(3).locator('.value').textContent(),'Unknown');
  await page.evaluate(()=>{history.replaceState(null,'','#/worker');render();});
  assert.equal(await page.getByText('Automatic worker updates',{exact:true}).count(),1);
  assert.match(await page.locator('#main').innerText(),/Manual updates remain the default/);
  assert.equal(await page.getByRole('button',{name:'Enable…',exact:true}).count(),1);
  assert.equal(await page.getByRole('button',{name:'Check status',exact:true}).count(),1);
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'offline update controls fit mobile');

  // A blocked historical offer must not poison current offers. Transient
  // worker checks do not remove the permission to declare a priced bid.
  const mixed = await page.evaluate(async () => {
    const original = {run, loadOffers, confirmDialog, offers:S.offers}; const calls=[];
    try {
      S.overview.initialized=true; S.offerFilter='all'; history.replaceState(null,'','#/models');
      const rows=[
        {model:'valid/a',name:'Valid',offered:true,authorized:true,pricingAvailable:true,offerable:true,localKeyConfigured:true,provider:'fal'},
        {model:'expired/w1',name:'Expired W1',offered:true,authorized:true,pricingAvailable:false,offerable:false,catalogGroup:'candidate'},
        {model:'transient/a',name:'Account check delayed',offered:true,authorized:true,pricingAvailable:true,offerable:false,workerAvailability:{status:'temporarily_unavailable'},localKeyConfigured:true}
      ];
      S.offers={offers:rows}; S.selectedOffers=new Set(rows.map(r=>r.model)); S.batchDiscount='5';
      loadOffers=async()=>{};confirmDialog=async()=>true;
      run=async(action,params,opts)=>{calls.push({action,params});return opts.prepareOnly?{state:'confirm',result:{question:'Review exact eligible models',hash:'reviewed'}}:{state:'done',result:{value:{}}};};
      await batchOffers(rows,false); const report=[...S.batchReport]; const submitted=calls[1].params.models;
      const canChangeTransient=canApplyOffer(rows[2]); calls.length=0;
      S.selectedOffers=new Set(['expired/w1']); await batchOffers(rows,true);
      return {report,submitted,canChangeTransient,withdraw: calls[1].params};
    } finally {run=original.run;loadOffers=original.loadOffers;confirmDialog=original.confirmDialog;S.offers=original.offers;S.selectedOffers.clear();clearBatchReport();}
  });
  assert.equal(mixed.submitted,'valid/a,transient/a');
  assert.equal(mixed.report.filter(r=>r.ok).length,2);
  assert(mixed.report.some(r=>r.model==='Expired W1'&&r.skipped&&/current price/.test(r.message)));
  assert.equal(mixed.canChangeTransient,true);
  assert.equal(mixed.withdraw.models,'expired/w1');assert.equal(mixed.withdraw.withdraw,true);

  await page.evaluate(()=>{
    S.overview.initialized=true;S.overview.deployment={phase:'running',appId:'1'.repeat(40)};
    S.health={state:'done',result:{value:{cloudStatus:'stopped',checks:[{name:'Accepting new work',ok:false}]}}};
    history.replaceState(null,'','#/overview');render();
  });
  assert.match(await page.locator('#meta').innerText(),/Stopped/);
  assert.doesNotMatch(await page.locator('#meta').innerText(),/Running/);
  await page.evaluate(()=>{S.health={state:'done',result:{value:{checks:[]}}};render();});
  assert.match(await page.locator('#meta').innerText(),/Checking state/);
  await page.evaluate(()=>{S.overview.deployment.phase='deleted';S.health=null;history.replaceState(null,'','#/worker');render();});
  assert(await page.getByRole('button',{name:'Apply keys',exact:true}).isDisabled());
  assert(await page.getByRole('button',{name:'Shut down',exact:true}).isDisabled());

  await page.evaluate(()=>{
    window.reliabilityOriginals={loadOverview,loadBalances,api};
    S.overview.keys=[{provider:'pika',kind:'generation',saved:false},{provider:'openrouter',kind:'generation',saved:true}];
    S.overview.deployment={phase:'not_deployed'};
    loadOverview=async()=>S.overview;loadBalances=async()=>{};
    api=async(path)=>{if(path==='/api/keys'){S.overview.keys[0].saved=true;return {ok:true,data:{check:{result:'valid'}}};}return window.reliabilityOriginals.api(path);};
    history.replaceState(null,'','#/keys');render();
  });
  await page.getByLabel('OpenRouter key',{exact:true}).fill('unsaved-fixture-draft');
  await page.getByLabel('Pika key',{exact:true}).fill('pika-fixture-not-a-real-key');
  await page.getByLabel('Pika key',{exact:true}).locator('..').getByRole('button',{name:'Save',exact:true}).click();
  await page.waitForFunction(()=>S.keyStatus.pika?.kind==='ok');
  assert.equal(await page.getByLabel('OpenRouter key',{exact:true}).inputValue(),'unsaved-fixture-draft');
  await page.evaluate(()=>{({loadOverview,loadBalances,api}=window.reliabilityOriginals);});

  await page.evaluate(()=>{
    S.overview.initialized=false;S.health=null;S.overview.steps=['hotkey','phala','providers','init','deploy','activate','offers','resume'].map((id,i)=>({id,done:i<2}));
    S.openStep='providers';history.replaceState(null,'','#/setup');render();
  });
  assert.equal(await page.getByLabel('Pika key',{exact:true}).count(),1);
  assert.equal(await page.getByLabel('Kling (direct) key',{exact:true}).count(),1);
  await page.getByLabel('Pika key',{exact:true}).fill('draft-kept-while-progress-changes');
  await page.evaluate(()=>{S.overview.steps[2].done=true;patchSetupProgress();});
  assert.equal(await page.locator('[data-setup-progress]').innerText(),'3 of 8 steps done');
  assert.equal(await page.getByLabel('Pika key',{exact:true}).inputValue(),'draft-kept-while-progress-changes');

  await page.evaluate(()=>{
    S.overview.initialized=true;S.offerFilter='all';S.modelQuery='';S.modelProvider='';S.modelKind='';
    S.offers={offers:[{model:'draft/a',name:'Draft A',provider:'fal',kind:'video',offered:true,discountBp:1000,offerable:true},{model:'draft/b',name:'Draft B',provider:'pika',kind:'image',offerable:true}]};
    history.replaceState(null,'','#/models');render();
  });
  await page.getByLabel('Discount percent for Draft A',{exact:true}).fill('12.5');
  await page.getByLabel('Select Draft B',{exact:true}).check();
  assert.equal(await page.getByLabel('Discount percent for Draft A',{exact:true}).inputValue(),'12.5');
  await page.getByLabel('Search models',{exact:true}).fill('Draft B');
  assert.equal(await page.getByLabel('Select Draft A',{exact:true}).count(),0);
  assert.equal(await page.getByLabel('Select Draft B',{exact:true}).count(),1);
  await page.getByLabel('Search models',{exact:true}).fill('');
  await page.getByLabel('Filter model provider',{exact:true}).selectOption('fal');
  assert.equal(await page.getByLabel('Select Draft B',{exact:true}).count(),0);

  const recovery=await page.evaluate(async()=>{
    const original={api,refreshAll};let posts=0,gets=0;
    try{
      S.pendingOperations.clear();refreshAll=async()=>{};
      api=async(path)=>{if(path==='/api/run'){posts++;return {ok:false,status:0,data:{}};}gets++;return {ok:true,data:{state:'done',result:{value:{phase:'running'}}}};};
      const unknown=await run('worker-update',{}, {quiet:true});const id=unknown.operationId;
      const saved=JSON.parse(sessionStorage.getItem('everycli-pending-operations'));
      S.pendingOperations.clear();restorePendingOperations();const restored=S.pendingOperations.size;
      await checkOperation(id);
      return {state:unknown.state,id,posts,gets,restored,remaining:S.pendingOperations.size,savedFields:Object.keys(saved.operations[0]).sort()};
    }finally{api=original.api;refreshAll=original.refreshAll;S.pendingOperations.clear();savePendingOperations();document.querySelector('#activity').replaceChildren();}
  });
  assert.equal(recovery.state,'unknown');assert.match(recovery.id,/^[a-f0-9]{32}$/);
  assert.equal(recovery.posts,1,'recovery never resubmits the mutation');assert.equal(recovery.gets,1);
  assert.equal(recovery.restored,1);assert.equal(recovery.remaining,0);
  assert(!recovery.savedFields.includes('params')&&!recovery.savedFields.includes('approvals'));

  const pollRecovery=await page.evaluate(async()=>{
    const original={api,refreshAll};let posts=0,reads=0,recovered=false;
    try{
      refreshAll=async()=>{};
      api=async(path,body)=>{
        if(path==='/api/run'){posts++;return {ok:true,data:{job:body.requestId}};}
        reads++;
        return recovered?{ok:true,data:{state:'done',result:{value:{phase:'running'}}}}:{ok:false,status:0,data:{}};
      };
      const job=await run('start',{maxHourlyUsd:'10'});const id=job.operationId;
      const banner=!!document.querySelector('#pending-operations');
      recovered=true;await checkOperation(id);
      return {state:job.state,posts,reads,banner,remaining:S.pendingOperations.size};
    }finally{api=original.api;refreshAll=original.refreshAll;S.pendingOperations.clear();savePendingOperations();patchPendingBanner();document.querySelector('#activity').replaceChildren();}
  });
  assert.equal(pollRecovery.state,'unknown');assert.equal(pollRecovery.posts,1);
  assert.equal(pollRecovery.reads,4,'retry reads before retaining an unconfirmed outcome');
  assert.equal(pollRecovery.banner,true);assert.equal(pollRecovery.remaining,0);

  await page.evaluate(async()=>{
    const original=run;
    try{S.overview.deployment={phase:'running',appId:'1'.repeat(40)};history.replaceState(null,'','#/worker');render();
      run=async()=>({state:'done',result:{value:{enabled:true,automaticUpdates:{enabled:true,report:{state:'idle'}}}}});await loadWorkerAuto();
    }finally{run=original;}
  });
  assert.match(await page.locator('#worker-auto-status').innerText(),/Automatic \(in VM\)/);
  assert(await page.getByRole('button',{name:'Enable…',exact:true}).isDisabled());
  assert(await page.getByRole('button',{name:'Disable',exact:true}).isEnabled());

  const workerUpdate=await page.evaluate(async()=>{
    const originals={run,refreshAll};const calls=[];
    try{
      S.workerAuto={state:'done',result:{value:{automaticUpdates:{enabled:false}}}};
      S.workerUpdate=null;S.workerUpdateAt=0;
      S.overview.deployment={phase:'running',appId:'1'.repeat(40)};
      refreshAll=async()=>{};
      run=async(action)=>{calls.push(action);return {state:'done',result:{value:action==='worker-update-check'
        ?{state:'available',release:'signed-test-release'}:action==='worker-update-abort'
        ?{state:'aborted'}:{state:'complete',release:'signed-test-release'}}};};
      await checkWorkerUpdate(true);render();
      const before=document.querySelector('#worker-software').innerText;
      await changeWorkerUpdate();const after=document.querySelector('#worker-software').innerText;
      S.workerUpdate={state:'attention',result:{value:{state:'waiting',reason:'Waiting for current jobs'}}};patchWorkerUpdate();
      const canContinue=!Array.from(document.querySelectorAll('#worker-software button')).find(b=>b.textContent==='Continue worker update').disabled;
      await changeWorkerUpdate(true);
      return {calls,before,after,canContinue};
    }finally{run=originals.run;refreshAll=originals.refreshAll;}
  });
  assert.match(workerUpdate.before,/Worker update available/);
  assert.match(workerUpdate.after,/Worker update complete/);
  assert.equal(workerUpdate.canContinue,true);
  assert.deepEqual(workerUpdate.calls,['worker-update-check','worker-update','worker-update-abort']);
  const reasons=await page.evaluate(()=>({
    stale:readiness({providerAvailability:{status:'temporarily_unavailable',checkedAt:Date.now()-7200000}})[0],
    offline:readiness({providerAvailability:{status:'temporarily_unavailable',checkedAt:Date.now()-7200000},blockers:['worker_offline']})[0],
    key:readiness({workerAvailability:{status:'access_denied'}})[0]
  }));
  assert.equal(reasons.stale,'Provider catalog check is stale');
  assert.equal(reasons.offline,'Worker offline');
  assert.equal(reasons.key,'Provider access unavailable for worker key');

  await page.evaluate(()=>{
    S.workerUpdate={state:'done',result:{value:{state:'available',release:'signed-test-release'}}};
    S.pendingOperations.set('a'.repeat(32),{id:'a'.repeat(32),action:'worker-update',unconfirmed:true});render();
  });
  assert(await page.getByRole('button',{name:'Update worker',exact:true}).isDisabled(),'unknown operation cannot be repeated');
  await page.evaluate(()=>{S.pendingOperations.clear();savePendingOperations();});

  for (const width of [1440,390]) {
    await page.setViewportSize({width,height:900});
    for (const route of ['models','worker']) {
      await page.evaluate(route=>{history.replaceState(null,'','#/'+route);render();},route);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,route+' fits '+width);
      if(process.env.EVERYCLI_QA_DIR)await page.screenshot({path:join(process.env.EVERYCLI_QA_DIR,route+'-fixed-'+width+'.png'),fullPage:true});
    }
  }
  assert.deepEqual(errors, []);
  console.log('UI browser regressions passed: pairing, drafts, lifecycle, readiness, W credits, diagnostics, history filters/pagination/details/races, earnings, mixed-offer batches, draft preservation, operation recovery and mobile.');
} finally {
  if (browser) await browser.close();
  if (cli.exitCode === null) { const stopped = once(cli, 'exit'); cli.kill('SIGTERM'); await stopped; }
  await rm(scratch, { recursive: true, force: true });
}

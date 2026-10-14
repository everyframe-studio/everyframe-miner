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
  await page.getByRole('textbox', { name: 'Local worker label' }).fill('unsaved-label');
  await page.getByRole('heading', { name: 'Worker', exact: true }).click();
  await page.evaluate(async () => { await window.reviewIntervals.find(x => x.ms === 30000).fn(); });
  assert.equal(await page.getByRole('textbox', { name: 'Local worker label' }).inputValue(), 'unsaved-label');

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

  await page.evaluate(() => { window.confirmResult = null; confirmDialog('Test question only', 'Test').then(v => window.confirmResult = v); });
  assert.equal(await page.locator('#app').evaluate(e => e.inert), true);
  for (let i = 0; i < 6; i++) await page.keyboard.press('Tab');
  assert.equal(await page.evaluate(() => !!document.activeElement.closest('#modal')), true);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => window.confirmResult), false);
  assert.equal(await page.locator('#app').evaluate(e => e.inert), false);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => { history.replaceState(null, '', '#/setup'); render(); });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'mobile viewport must not overflow');
  assert.deepEqual(errors, []);
  console.log('UI browser regressions passed: pairing, drafts, lifecycle, readiness, W credits, diagnostics, modal and mobile.');
} finally {
  if (browser) await browser.close();
  if (cli.exitCode === null) { const stopped = once(cli, 'exit'); cli.kill('SIGTERM'); await stopped; }
  await rm(scratch, { recursive: true, force: true });
}

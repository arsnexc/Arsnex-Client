// Run:  cd prototype && python3 -m http.server 8931 &
//       npm i puppeteer && node tools/insttest.mjs
// (needs the puppeteer OS deps: libnspr4 libnss3 libasound2t64
//  libatk-bridge2.0-0 libgtk-3-0 libgbm1)
//
// Instance-selection regression test — guards the v2.5.2 fix for the
// hardcoded instance 'main' (LAUNCH / My Mods scan / installs).
//
// Runs the real prototype/arsex.html in headless Chrome with the native
// bridge surface stubbed BEFORE the app script runs, then asserts the UI
// launches the instance the user selected.
//
//   node /home/user/insttest.mjs
import puppeteer from 'puppeteer';

const URL = process.env.ARSEX_TEST_URL || 'http://127.0.0.1:8931/arsex.html';
let failures = 0;
const ok = (cond, label) => {
  console.log((cond ? '  PASS  ' : '  FAIL  ') + label);
  if (!cond) failures++;
};

const INSTANCES = [
  { slug: 'main', name: 'Main', version: '1.20.4', loader: 'Fabric', memory: 4096, last_played: 10 },
  { slug: 'pvp',  name: 'PvP',  version: '1.20.4', loader: 'Fabric', memory: 2048, last_played: 9999 },
];

const browser = await puppeteer.launch({
  headless: 'new',
  args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
});

async function freshPage(instances, withLaunch = true) {
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  await page.evaluateOnNewDocument((insts, withLaunch) => {
    // The exact surface the native bridge provides, stubbed.
    window.__listInstances = async () => insts;
    window.__launchCalls = [];
    if (withLaunch) {
      window.__nativeLaunch = async (opts) => {
        window.__launchCalls.push(opts);
        return 4242;
      };
    }
    window.__scanMods = async (instance, loader) => {
      window.__scanCalls = window.__scanCalls || [];
      window.__scanCalls.push({ instance, loader });
      return { mods: [], problems: [], unreadable: [] };
    };
  }, instances, withLaunch);
  await page.goto(URL, { waitUntil: 'networkidle0' });
  // The boot cinematic covers the UI for ~4.3s; a real user waits it out,
  // the test does too.
  await page.waitForFunction(
    () => document.getElementById('boot')?.classList.contains('gone'),
    { timeout: 15000 });
  await page.waitForFunction(() => document.querySelector('.verrow .chip'));
  return page;
}

// ---- 1. chips render from real instances; 'main' wins the default -------
{
  const page = await freshPage(INSTANCES);
  const chips = await page.$$eval('.verrow [data-inst]', els =>
    els.map(e => ({ slug: e.dataset.inst, on: e.classList.contains('on') })));
  ok(chips.length === 2, `instance chips rendered (${chips.length} of 2)`);
  ok(chips.find(c => c.slug === 'main')?.on === true, "'main' is the default active chip");
  ok(chips.find(c => c.slug === 'pvp')?.on === false, "'pvp' not active initially");
  const sub = await page.$eval('.bigplay .sub2', e => e.textContent);
  ok(/1\.20\.4\s*·\s*FABRIC/i.test(sub), `LAUNCH sub-label reflects instance (${sub})`);
  const mem = await page.$eval('#memMeta', e => e.textContent);
  ok(/4 GB/.test(mem), `memory label reflects instance (${mem})`);
  await page.close();
}

// ---- 2. clicking a chip switches the launch target -----------------------
{
  const page = await freshPage(INSTANCES);
  await page.click('.verrow [data-inst="pvp"]');
  await page.waitForFunction(() => {
    const m = document.getElementById('memMeta');
    return m && /2 GB/.test(m.textContent);
  });
  const active = await page.evaluate(() => INST.active && INST.active.slug);
  ok(active === 'pvp', `INST.active switched to 'pvp' (${active})`);
  await page.click('#play');
  await page.waitForFunction(() => window.__launchCalls.length > 0, { timeout: 15000 });
  const call = (await page.evaluate(() => window.__launchCalls[0])) || {};
  ok(call.instance === 'pvp', `LAUNCH used the selected slug (${call.instance})`);
  ok(call.version === '1.20.4', `LAUNCH used the instance's real version (${call.version})`);
  ok(call.memory === 2048, `LAUNCH used the instance's memory (${call.memory})`);
  await page.close();
}

// ---- 3. no 'main' -> most-recently-played becomes the target -------------
{
  const page = await freshPage([INSTANCES[1]]);
  const active = await page.evaluate(() => INST.active && INST.active.slug);
  ok(active === 'pvp', `only-instance becomes active (${active})`);
  await page.click('#play');
  await page.waitForFunction(() => window.__launchCalls.length > 0, { timeout: 15000 });
  const call = (await page.evaluate(() => window.__launchCalls[0])) || {};
  ok(call.instance === 'pvp', `LAUNCH used it (${call.instance})`);
  await page.close();
}

// ---- 4. no instances at all -> LAUNCH refuses, opens the wizard ----------
{
  const page = await freshPage([], true);
  // Scoped to the hero: the settings page has a second .verrow (themes).
  const chips = await page.$$eval('.launchcol .verrow .chip', els => els.length);
  ok(chips === 4, `static preview chips kept when no instances (${chips})`);
  await page.click('#play');
  await new Promise(r => setTimeout(r, 1500));
  const calls = await page.evaluate(() => window.__launchCalls.length);
  ok(calls === 0, `no phantom launch without an instance (${calls} calls)`);
  const wizOpen = await page.evaluate(() => !!document.querySelector('#iName'));
  ok(wizOpen, 'wizard opened instead');
  await page.close();
}

// ---- 5. My Mods scans the ACTIVE instance, not 'main' --------------------
{
  const page = await freshPage([INSTANCES[1]]);
  await page.evaluate(() => mmSyncNative());
  await page.waitForFunction(() => (window.__scanCalls || []).length > 0);
  const scan = await page.evaluate(() => window.__scanCalls[0]);
  ok(scan.instance === 'pvp', `My Mods scanned the active instance (${scan.instance})`);
  ok(scan.loader === 'fabric', `loader passed through (${scan.loader})`);
  await page.close();
}

await browser.close();
console.log(failures === 0 ? '\nALL INSTANCE TESTS PASSED' : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

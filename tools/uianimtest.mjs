// UI motion + UX regression test — guards the v2.5.3 animation pass.
//
//   cd prototype && python3 -m http.server 8931 &
//   ARSEX_TEST_URL=http://127.0.0.1:8931/arsex.html node tools/uianimtest.mjs
//
// Covers: boot skip, OS reduced-motion honoured end-to-end (CSS + countUp),
// the real-progress hero bar (heroStage), card hover physics, and the
// value-tick when the selected instance changes.
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

async function freshPage({ reduceMotion = false } = {}) {
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  if (reduceMotion) {
    await page.emulateMediaFeatures([
      { name: 'prefers-reduced-motion', value: 'reduce' },
    ]);
  }
  await page.evaluateOnNewDocument((insts) => {
    window.__listInstances = async () => insts;
    window.__nativeLaunch = async (o) => { window.__launchCalls = (window.__launchCalls || []).concat([o]); return 1; };
  }, INSTANCES);
  await page.goto(URL, { waitUntil: 'networkidle0' });
  return page;
}

// ---- 1. boot cinematic is skippable --------------------------------------
{
  const page = await freshPage();
  const early = await page.evaluate(
    () => !document.getElementById('boot').classList.contains('gone'));
  ok(early, 'boot cinematic still up after load (skip is a real action)');
  await page.click('#boot');
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 2000 });
  ok(true, 'click on boot skips it immediately');
  // The app fades in over ~.55s once revealed; wait the transition out
  // instead of sampling opacity at the instant of the click.
  await page.waitForFunction(
    () => parseFloat(getComputedStyle(document.getElementById('app')).opacity) > 0.5,
    { timeout: 4000 });
  ok(true, 'app visible after skip');
  await page.close();
}

// ---- 2. OS reduced-motion is honoured ------------------------------------
{
  const page = await freshPage({ reduceMotion: true });
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 15000 });
  const flagged = await page.evaluate(
    () => document.documentElement.dataset.motion === 'reduced');
  ok(flagged, 'html[data-motion="reduced"] set from the OS preference');
  const dur = await page.evaluate(() => {
    const el = document.querySelector('.ncard') || document.querySelector('.stat');
    return getComputedStyle(el).animationDuration;
  });
  ok(dur === '0.001s', `CSS animations collapsed (duration ${dur})`);
  const snapped = await page.evaluate(() => {
    const el = document.querySelector('[data-count]');
    return el && el.textContent === (+el.dataset.count).toLocaleString();
  });
  ok(snapped, 'countUp snaps to final values instead of animating');
  await page.close();
}

// ---- 3. hero bar reflects REAL stage events ------------------------------
{
  const page = await freshPage();
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 15000 });
  await page.evaluate(() => heroStage({ pct: 37, label: 'Downloading libraries', detail: '12/48 files' }));
  const st = await page.evaluate(() => {
    const bar = document.getElementById('launchBar');
    return {
      on: bar.classList.contains('on'),
      w: bar.querySelector('i').style.width,
      pct: bar.querySelector('b').textContent,
      sub: document.querySelector('.bigplay .sub2').textContent,
    };
  });
  ok(st.on, 'hero bar visible during a launch');
  ok(st.w === '37%', `fill width tracks the real pct (${st.w})`);
  ok(st.pct === '37%', `numeric readout tracks (${st.pct})`);
  ok(/Downloading libraries/.test(st.sub) && /12\/48/.test(st.sub),
     `stage label + detail on the button (${st.sub})`);
  await page.evaluate(() => heroStage({ pct: 100, label: 'Starting JVM', detail: '' }));
  await page.waitForFunction(
    () => !document.getElementById('launchBar').classList.contains('on'),
    { timeout: 3000 });
  ok(true, 'bar auto-hides after completion');
  await page.close();
}

// ---- 4. card hover physics ------------------------------------------------
{
  const page = await freshPage();
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 15000 });
  const before = await page.$eval('.stat', e => getComputedStyle(e).transform);
  await page.hover('.stat');
  await new Promise(r => setTimeout(r, 260));
  const after = await page.$eval('.stat', e => getComputedStyle(e).transform);
  ok(before === 'none' && after !== 'none',
     `telemetry card lifts on hover (${before} -> ${after})`);
  await page.close();
}

// ---- 5. instance switch ticks values in ----------------------------------
{
  const page = await freshPage();
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 15000 });
  await page.click('.verrow [data-inst="pvp"]');
  const ticked = await page.evaluate(() =>
    document.querySelector('.bigplay .sub2').classList.contains('vtick'));
  ok(ticked, 'LAUNCH sub-label ticks in on instance change');
  await page.close();
}

await browser.close();
console.log(failures === 0 ? '\nALL MOTION TESTS PASSED' : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

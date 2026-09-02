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

// ---- living background: ink field, contours, parallax --------------------
{
  const page = await freshPage();
  const layers = await page.evaluate(() => ({
    blobs: document.querySelectorAll('#veilInk i').length,
    wash: !!document.querySelector('#veilInk .wash'),
    topo: !!document.getElementById('veilTopo'),
    drift: getComputedStyle(document.querySelector('#veilInk i:nth-child(1)')).animationName,
  }));
  ok(layers.blobs === 3 && layers.wash, `ink field: 3 drifting masses + wash (${layers.blobs})`);
  ok(layers.topo, 'contour line layer present');
  ok(layers.drift === 'ink1', `drift keyframe applied (${layers.drift})`);

  // Parallax: cursor movement eases the field away from centre.
  await page.mouse.move(1200, 250);
  await page.waitForFunction(() => {
    const m = /translate3d\((-?\d+\.?\d*)px, (-?\d+\.?\d*)px, 0px\)/
      .exec(document.getElementById('veilInk').style.transform);
    return m && (Math.abs(+m[1]) > 1.5 || Math.abs(+m[2]) > 1.5);
  }, { timeout: 4000 });
  const moved = await page.evaluate(() => ({
    ink: document.getElementById('veilInk').style.transform,
    topo: document.getElementById('veilTopo').style.transform,
  }));
  ok(/translate3d\(-?\d+\.?\d*px/.test(moved.ink) && !/translate3d\(0\.00px, 0\.00px/.test(moved.topo),
    `parallax eases against the cursor (${moved.ink} · ${moved.topo})`);

  // Reduced motion: drift collapses AND further pointer movement is ignored.
  // Wait for the eased follow to converge BEFORE freezing the state.
  await new Promise(r => setTimeout(r, 900));
  await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
  await new Promise(r => setTimeout(r, 120));
  const still = await page.evaluate(() => {
    const el = document.querySelector('#veilInk i:nth-child(1)');
    const cs = getComputedStyle(el);
    return { a: cs.animationName, d: cs.animationDuration,
             t: document.getElementById('veilInk').style.transform };
  });
  await page.mouse.move(150, 800);
  await new Promise(r => setTimeout(r, 400));
  const t2 = await page.evaluate(() => document.getElementById('veilInk').style.transform);
  const collapsed = still.a === 'none' || parseFloat(still.d) < 0.01;
  ok(collapsed && t2 === still.t,
    `reduced motion: drift collapsed (${still.a}/${still.d}), parallax frozen`);
  await page.close();
}

// ---- chip entrance stagger ----------------------------------------------
{
  const page = await freshPage();
  const delays = await page.$$eval('.launchcol .verrow .chip',
    els => els.map(e => getComputedStyle(e).animationDelay));
  ok(delays.length >= 2 && delays[0] === '0s' && parseFloat(delays[1]) > 0,
    `chips wave in (${delays.join(', ')})`);
  await page.close();
}

// ---- press feedback + keyboard focus visibility --------------------------
{
  const page = await freshPage();
  // Skip the boot cinematic first: it is a full-screen overlay at z-index
  // 500, and a press behind it would target the boot screen, not the chip.
  await page.click('#boot');
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'),
    { timeout: 4000 });
  const chip = await page.$('.launchcol .verrow .chip');
  const box = await chip.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down(); // hold the press
  await new Promise(r => setTimeout(r, 80));
  const pressed = await page.evaluate(() => {
    const c = document.querySelector('.launchcol .verrow .chip');
    return { k: c.classList.contains('pressing'), t: getComputedStyle(c).transform };
  });
  await page.mouse.up();
  await new Promise(r => setTimeout(r, 120));
  const released = await page.evaluate(() => {
    const c = document.querySelector('.launchcol .verrow .chip');
    return { k: c.classList.contains('pressing'), t: getComputedStyle(c).transform };
  });
  ok(pressed.k && pressed.t !== 'none',
    `press feedback while held (${pressed.k} · ${pressed.t})`);
  ok(!released.k, 'press feedback released');
  // The stylesheet must carry a :focus-visible rule for keyboard users.
  const hasFocusRule = await page.evaluate(() => {
    for (const sheet of document.styleSheets)
      try { for (const r of sheet.cssRules)
        if (r.selectorText && r.selectorText.includes(':focus-visible')) return true; } catch (e) {}
    return false;
  });
  ok(hasFocusRule, ':focus-visible rule exists (keyboard sightline)');
  await page.close();
}

// ---- background v2: ink motes + focus hold ------------------------------
{
  const page = await freshPage();
  await page.click('#boot');
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'), { timeout: 4000 });
  const motes = await page.evaluate(() => ({
    canvas: !!document.getElementById('veilMotes'),
    ...(window.__motes || {}),
  }));
  ok(motes.canvas && motes.running === true && motes.n >= 40,
    `ink motes running (${motes.n} particles, running=${motes.running})`);

  // Focus hold: opening the wizard pauses the whole scene.
  await page.click('#newInst');
  await page.waitForFunction(() => document.body.classList.contains('modal-open'));
  const held = await page.evaluate(() => ({
    ink: getComputedStyle(document.querySelector('#veilInk i')).animationPlayState,
    motes: window.__motes.running,
  }));
  ok(held.ink === 'paused' && held.motes === false,
    `modal open: ink paused (${held.ink}), motes paused (${held.motes})`);
  await page.keyboard.press('Escape');
  await page.waitForFunction(() => !document.body.classList.contains('modal-open'),
    { timeout: 3000 }).catch(() => {});
  const resumed = await page.evaluate(() => window.__motes.running);
  ok(resumed === true, 'modal closed: scene resumes');
  await page.close();
}

// ---- reduced motion: motes draw once and stop ----------------------------
{
  const page = await freshPage({ reduceMotion: true });
  await page.waitForFunction(
    () => document.getElementById('boot').classList.contains('gone'), { timeout: 15000 });
  const still = await page.evaluate(() => window.__motes || {});
  ok(still.running === false, `reduced motion: mote engine stopped (running=${still.running})`);
  await page.close();
}

await browser.close();
console.log(failures === 0 ? '\nALL MOTION TESTS PASSED' : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

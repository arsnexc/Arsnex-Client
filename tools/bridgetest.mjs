// bridgetest.mjs — proves the NATIVE BRIDGE (v2.6.1) actually wires the UI to
// the Tauri backend. Everything else in the app was already written against
// `window.__nativeLaunch` / `window.__createInstance` / … guards; until this
// suite existed, nothing defined those functions and the packaged app ran in
// browser-preview mode (no installs, scripted LAUNCH theatre).
//
// Method: inject a mock `window.__TAURI__` global BEFORE the page loads. The
// mock records every invoke, answers with canned backend responses, and
// captures event registrations so tests can fire real event payloads
// (launch://stage, instance://stage, game://log, …) at the UI.
//
// Run: node tools/bridgetest.mjs   (static server on :8931, ARSEX_TEST_URL overrides)

import puppeteer from 'puppeteer';

const URL = process.env.ARSEX_TEST_URL || 'http://127.0.0.1:8931/arsex.html';
let failures = 0;
const ok = (cond, label) => {
  console.log(`  ${cond ? 'PASS' : 'FAIL'}  ${label}`);
  if (!cond) failures++;
};

const INSTANCES = [
  { slug: 'main', name: 'Main 1.20.4', icon: 0, version: '1.20.4', loader: 'Fabric',
    memory: 4096, isolate_saves: true, discord_rpc: false, created: 1, last_played: 2 },
  { slug: 'pvp', name: 'PvP 1.16.5', icon: 1, version: '1.16.5', loader: 'Fabric',
    memory: 2048, isolate_saves: false, discord_rpc: false, created: 1, last_played: 3 },
];

// The mock backend. Handles are per-test overridable via window.__mock.handles.
const MOCK = () => {
  const calls = [];
  const listeners = {};
  window.__mock = {
    calls,
    handles: {
      list_instances: () => JSON.parse(JSON.stringify(window.__mockInstances || [])),
      current_account: () => JSON.parse(JSON.stringify(window.__mockAccount || null)),
      scan_mods: () => ({ mods: [], problems: [], unreadable: [] }),
      launch_game: () => 4242,
      create_instance: a => ({ slug: 'brand-new', name: a.req.name, icon: a.req.icon,
        version: a.req.version, loader: a.req.loader, memory: a.req.memory,
        isolate_saves: a.req.isolate_saves, discord_rpc: a.req.discord_rpc,
        created: 9, last_played: 0 }),
    },
    fire: (ev, payload) => (listeners[ev] || []).forEach(f => f({ payload })),
  };
  const invoke = async (cmd, args) => {
    calls.push({ cmd, args });
    const h = window.__mock.handles[cmd];
    if (h) return h(args);
    throw new Error(`unmocked command: ${cmd}`);
  };
  const listen = (ev, fn) => { (listeners[ev] = listeners[ev] || []).push(fn); return Promise.resolve(() => {}); };
  Object.defineProperty(window, '__TAURI__', {
    value: { core: { invoke }, event: { listen } }, configurable: true,
  });
};

const browser = await puppeteer.launch({ args: ['--no-sandbox', '--disable-gpu'] });

async function freshPage(instances, account, withTauri = true) {
  const page = await browser.newPage();
  if (withTauri) {
    await page.evaluateOnNewDocument(MOCK);
    if (instances) await page.evaluateOnNewDocument(list => { window.__mockInstances = list; }, instances);
    if (account) await page.evaluateOnNewDocument(a => { window.__mockAccount = a; }, account);
  }
  await page.goto(URL, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => document.querySelector('#boot.gone'), { timeout: 15000 });
  await new Promise(r => setTimeout(r, 250));
  return page;
}
const calls = page => page.evaluate(() => window.__mock.calls);
const cmds = async (page, name) => (await calls(page)).filter(c => c.cmd === name);

// ---- 1. without __TAURI__ the bridge must NOT activate (preview stays honest)
{
  const page = await freshPage(null, null, false);
  const absent = await page.evaluate(() =>
    [window.__createInstance, window.__nativeLaunch, window.__listInstances,
     window.__scanMods, window.__currentAccount].every(f => typeof f === 'undefined'));
  ok(absent, 'no Tauri global -> bridge stays inactive (preview fallbacks intact)');
  await page.close();
}

// ---- 2. bridge activates + boot pulls real instances + account ----------
{
  const page = await freshPage(INSTANCES, { username: 'Tester', uuid: 'u-1', owns_game: true });
  ok((await cmds(page, 'list_instances')).length >= 1, 'boot invoked list_instances');
  const chips = await page.$$eval('.launchcol .verrow .chip', els => els.map(e => e.textContent));
  ok(chips.some(t => /Main 1\.20\.4/.test(t)) && chips.some(t => /PvP 1\.16\.5/.test(t)),
    `hero chips come from the backend (${chips.join(' | ')})`);
  const acct = await page.$eval('#accts .aname', e => e.textContent).catch(() => null);
  ok(acct === 'Tester', `current_account rendered (${acct})`);
  ok((await cmds(page, 'scan_mods')).length >= 1, 'boot scanned the active instance mods');
  await page.close();
}

// ---- 3. wizard create: exact payload, live stages, refresh ---------------
{
  const page = await freshPage(INSTANCES, null);
  // Hold the backend call open so the in-flight stages are observable.
  await page.evaluate(() => {
    window.__mock.handles.create_instance = a => new Promise(res => { window.__resolveCreate = res; });
  });
  await page.click('#newInst');
  await page.waitForFunction(() => !!document.querySelector('#iName'));
  await page.type('#iName', 'Brand New');
  for (let i = 0; i < 4; i++) { await page.click('#mNext'); await new Promise(r => setTimeout(r, 220)); }
  await page.waitForFunction(() => document.querySelector('.creating'));
  const sent = (await cmds(page, 'create_instance'))[0];
  ok(!!sent, 'create_instance invoked');
  ok(sent.args.req.copy_config_from === 'main',
    `copy_config_from carries the active instance slug (${sent.args.req.copy_config_from})`);
  ok(sent.args.req.name === 'Brand New' && sent.args.req.version === '1.20.4'
     && sent.args.req.loader === 'Fabric' && sent.args.req.memory >= 1024
     && typeof sent.args.req.isolate_saves === 'boolean',
    `payload shape matches NewInstance (${JSON.stringify(sent.args.req)})`);
  // Real backend progress drives the overlay.
  await page.evaluate(() => window.__mock.fire('instance://stage',
    { key: 'libraries', label: 'Downloading libraries', pct: 44, detail: '12/48 files' }));
  await new Promise(r => setTimeout(r, 150));
  const ov = await page.evaluate(() => ({
    pct: document.querySelector('.cpct') ? document.querySelector('.cpct').textContent : '',
    log: document.querySelector('.clog') ? document.querySelector('.clog').textContent : '',
  }));
  ok(ov.pct === '44%' && /Downloading libraries/.test(ov.log),
    `instance://stage drives the overlay (${ov.pct} · ${ov.log})`);
  await page.evaluate(() => window.__resolveCreate(
    { slug: 'brand-new', name: 'Brand New', icon: 0, version: '1.20.4', loader: 'Fabric',
      memory: 4096, isolate_saves: true, discord_rpc: false, created: 9, last_played: 0 }));
  await new Promise(r => setTimeout(r, 900));
  const listed = (await cmds(page, 'list_instances')).length;
  ok(listed >= 2, `instance list refreshed after creation (${listed} invokes)`);
  await page.close();
}

// ---- 4. LAUNCH: exact payload, pid handoff, real stages, log stream ------
{
  const page = await freshPage(INSTANCES, { username: 'Tester', uuid: 'u-1', owns_game: true });
  // switch to the pvp instance so the launch args are unambiguous
  await page.evaluate(() => {
    const el = [...document.querySelectorAll('.launchcol .verrow .chip')].find(c => /PvP/.test(c.textContent));
    el.click();
  });
  await new Promise(r => setTimeout(r, 250));
  // Keep the pipeline "in flight" so its stage events are observable.
  await page.evaluate(() => {
    window.__mock.handles.launch_game = () => new Promise(res => { window.__resolveLaunch = res; });
  });
  await page.click('#play');
  await page.waitForFunction(() => (window.__mock.calls.filter(c => c.cmd === 'launch_game').length > 0));
  const sent = (await cmds(page, 'launch_game'))[0].args;
  ok(sent.instance === 'pvp' && sent.version === '1.16.5' && sent.player === 'Tester'
     && sent.uuid === 'u-1' && sent.token === '' && sent.java === null && sent.memory === 2048,
    `launch_game payload exact (${JSON.stringify(sent)})`);
  await new Promise(r => setTimeout(r, 200));
  await page.evaluate(() => window.__mock.fire('launch://stage',
    { key: 'loader', label: 'Installing Fabric loader', pct: 6, detail: 'fabric-loader 0.15.11' }));
  await new Promise(r => setTimeout(r, 200));
  const hero = await page.evaluate(() => {
    const bar = document.getElementById('launchBar');
    return { on: bar ? bar.classList.contains('on') : false,
             w: bar && bar.querySelector('i') ? bar.querySelector('i').style.width : '',
             pct: bar && bar.querySelector('b') ? bar.querySelector('b').textContent : '',
             sub: document.querySelector('.bigplay .sub2') ? document.querySelector('.bigplay .sub2').textContent : '' };
  });
  ok(hero.on && hero.w === '6%' && hero.pct === '6%'
     && /Installing Fabric loader/.test(hero.sub),
    `launch://stage drives the hero bar (${hero.on} ${hero.w} ${hero.pct} · ${hero.sub})`);
  await page.evaluate(() => window.__resolveLaunch(4242));
  await new Promise(r => setTimeout(r, 200));
  const before = await page.$eval('#conLines', e => +e.textContent);
  await page.evaluate(() => window.__mock.fire('game://log',
    { seq: 1, ts: '12:00:00', thread: 'main', level: 'INFO', msg: 'test line from the JVM' }));
  await new Promise(r => setTimeout(r, 150));
  const after = await page.$eval('#conLines', e => +e.textContent);
  ok(after === before + 1, `game://log lands in the console (${before} -> ${after})`);
  await page.evaluate(() => window.__mock.fire('game://exit', 0));
  await new Promise(r => setTimeout(r, 150));
  const exit = await page.$eval('#conExit', e => e.textContent);
  ok(exit === '0', `game://exit shown in the console (${exit})`);
  await page.close();
}

// ---- 5. mod problems surface as WARN console lines, not silence ----------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => window.__mock.fire('launch://mod-problem',
    { kind: 'version_mismatch', mod_id: 'arsex',
      detail: 'Arsex modules target Minecraft 1.20.4 — this instance runs 1.16.5' }));
  await new Promise(r => setTimeout(r, 150));
  const warns = await page.$eval('#conWarn', e => +e.textContent);
  ok(warns >= 1, `launch://mod-problem -> WARN line in console (${warns})`);
  await page.close();
}

// ---- 6. kill button reaches kill_game -----------------------------------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => { goto('console'); CON.attach(999); });
  await new Promise(r => setTimeout(r, 150));
  await page.click('#conKill');
  await new Promise(r => setTimeout(r, 250));
  ok((await cmds(page, 'kill_game')).length === 1, 'KILL invokes kill_game');
  await page.close();
}

// ---- 7. backend refusal words reach the wizard overlay -------------------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => {
    window.__mock.handles.create_instance = () => { throw new Error(
      'Fabric does not support Minecraft 1.8.9 (1.14 and newer only). Use the VANILLA loader for this version.'); };
  });
  await page.click('#newInst');
  await page.waitForFunction(() => !!document.querySelector('#iName'));
  await page.type('#iName', 'Doomed');
  for (let i = 0; i < 4; i++) { await page.click('#mNext'); await new Promise(r => setTimeout(r, 220)); }
  await page.waitForFunction(() => {
    const o = document.querySelector('.creating'); return o && o.classList.contains('err');
  }, { timeout: 5000 });
  const msg = await page.evaluate(() => document.querySelector('.clog').textContent);
  ok(/does not support Minecraft 1\.8\.9/.test(msg), `backend refusal text shown verbatim (${msg.slice(0, 60)}…)`);
  await page.close();
}

// ---- 8. failed launch resets the hero bar and surfaces the reason -------
{
  const page = await freshPage(INSTANCES, { username: 'Tester', uuid: 'u-1', owns_game: true });
  await page.evaluate(() => {
    // The real failure mode: the pipeline emits its loader stage, then the
    // fabric meta fetch dies. The bar must not stay frozen at that stage.
    window.__mock.handles.launch_game = () => new Promise((res, rej) =>
      setTimeout(() => rej(new Error(
        'could not reach fabric meta after 3 attempts (check your connection or a proxy blocking meta.fabricmc.net)')), 300));
  });
  await page.click('#play');
  await page.waitForFunction(() => window.__mock.calls.some(c => c.cmd === 'launch_game'));
  await page.evaluate(() => window.__mock.fire('launch://stage',
    { key: 'loader', label: 'Installing Fabric loader', pct: 6, detail: '' }));
  await new Promise(r => setTimeout(r, 800)); // rejection + reset + toast
  const state = await page.evaluate(() => {
    const bar = document.getElementById('launchBar');
    return { on: bar.classList.contains('on'),
             w: bar.querySelector('i').style.width,
             err: +document.getElementById('conErr').textContent,
             sub: document.querySelector('.bigplay .sub2').textContent };
  });
  ok(!state.on && (state.w === '0%' || state.w === ''),
    `failed launch resets the hero bar (on=${state.on} w=${state.w})`);
  ok(state.err >= 1, `failure reason visible as ERROR in the console (${state.err})`);
  await page.close();
}

// ---- 9. MANAGE: memory right-sizing reaches the backend ---------------
{
  const page = await freshPage(INSTANCES, null);
  await page.click('#instMgr');
  await page.waitForFunction(() => document.getElementById('mgScrim').classList.contains('on'));
  const title = await page.$eval('#mgTitle', e => e.textContent);
  ok(title === 'Main 1.20.4', `manage opens on the active instance (${title})`);
  await page.evaluate(() => {
    window.__memSet = null;
    window.__mock.handles.set_instance_memory = a => { window.__memSet = a;
      return { slug: a.slug, name: 'Main 1.20.4', icon: 0, version: '1.20.4', loader: 'Fabric',
        memory: a.memory, isolate_saves: true, discord_rpc: false, created: 1, last_played: 2 }; };
  });
  await page.evaluate(() =>
    [...document.querySelectorAll('#mgMems [data-g]')].find(e => e.dataset.g === '8').click());
  await page.click('#mgSave');
  await page.waitForFunction(() => window.__memSet);
  const sent = await page.evaluate(() => window.__memSet);
  ok(sent.slug === 'main' && sent.memory === 8192, `set_instance_memory payload (${JSON.stringify(sent)})`);
  await page.waitForFunction(() => !document.getElementById('mgScrim').classList.contains('on'));
  await page.close();
}

// ---- 10. MANAGE: delete demands a double confirm ------------------------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => {
    window.__deleted = 0;
    window.__mock.handles.delete_instance = () => { window.__deleted++; };
  });
  await page.click('#instMgr');
  await new Promise(r => setTimeout(r, 200));
  await page.click('#mgDel');
  await new Promise(r => setTimeout(r, 150));
  const armedState = await page.evaluate(() =>
    ({ n: window.__deleted, txt: document.getElementById('mgDel').textContent }));
  ok(armedState.n === 0 && /REALLY/.test(armedState.txt), 'first delete click only arms');
  await page.click('#mgDel');
  await page.waitForFunction(() => window.__deleted === 1);
  ok(true, 'second click performs the delete');
  await page.waitForFunction(() => !document.getElementById('mgScrim').classList.contains('on'));
  ok(true, 'modal closes after delete');
  await page.close();
}

// ---- 11. wizard: ALL RELEASES from the live manifest + fabric gate ------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => {
    window.__mock.handles.list_versions = () =>
      ['1.21.11', '1.21.10', '1.21.1', '1.20.6', '1.16.5', '1.12.2', '1.8.9'];
  });
  await page.click('#newInst');
  await page.waitForFunction(() => !!document.querySelector('#iName'));
  await page.type('#iName', 'Live Version');
  await page.click('#mNext'); // step 0 IDENTITY -> step 1 VERSION
  await new Promise(r => setTimeout(r, 250));
  await page.click('#verMore');
  await page.waitForFunction(() => document.getElementById('verAll').children.length > 0);
  const n = await page.evaluate(() => document.getElementById('verAll').children.length);
  ok(n === 7, `live release list rendered (${n})`);
  await page.evaluate(() => document.querySelector('#verAll [data-ver="1.21.1"]').click());
  await new Promise(r => setTimeout(r, 200));
  const fabOn = await page.evaluate(() => {
    const el = [...document.querySelectorAll('#loaderOpts .opt')].find(e => e.textContent.includes('Fabric'));
    return el ? !el.classList.contains('dis') : false;
  });
  ok(fabOn, 'Fabric available on 1.21.1 (predicate, not a stale lookup)');
  await page.evaluate(() => document.querySelector('#verAll [data-ver="1.12.2"]').click());
  await new Promise(r => setTimeout(r, 200));
  const fabOff = await page.evaluate(() => {
    const el = [...document.querySelectorAll('#loaderOpts .opt')].find(e => e.textContent.includes('Fabric'));
    return el ? el.classList.contains('dis') : false;
  });
  ok(fabOff, 'Fabric gated off on 1.12.2 from the live list too');
  await page.close();
}

// ---- 12. console: LOG FOLDER reaches open_log_dir -----------------------
{
  const page = await freshPage(INSTANCES, null);
  await page.evaluate(() => { goto('console'); });
  await page.click('#conLogs');
  await new Promise(r => setTimeout(r, 250));
  ok((await cmds(page, 'open_log_dir')).length === 1, 'LOG FOLDER invokes open_log_dir');
  await page.close();
}

await browser.close();
console.log(failures === 0 ? '\nALL BRIDGE TESTS PASSED' : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

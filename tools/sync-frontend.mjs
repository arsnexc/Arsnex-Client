#!/usr/bin/env node
// The prototype is the single source of truth for the UI. This copies it into
// launcher/dist/ as index.html and injects the native bridge, so the packaged
// exe and the browser prototype can never drift apart.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const src = readFileSync(resolve(root, 'prototype/arsex.html'), 'utf8');

// Built as an array of lines rather than a template literal: the bridge itself
// contains backticks and ${...}, and nesting those inside a JS template is how
// you get silently corrupted output.
const BRIDGE = [
  '',
  '/* ===== NATIVE BRIDGE (injected by tools/sync-frontend.mjs) ===== */',
  '(function(){',
  '  const T=window.__TAURI__;',
  "  if(!T){ document.documentElement.dataset.runtime='browser'; return; }",
  "  document.documentElement.dataset.runtime='native';",
  '  const {invoke}=T.core, {listen}=T.event;',
  '',
  '  // Real JVM output replaces the scripted emitter.',
  "  listen('game://log', e=>{const l=e.payload;CON.push(l.level,l.thread,l.msg)});",
  "  listen('game://exit', e=>CON.stop(e.payload));",
  "  listen('game://crash', e=>toast('Game crashed · exit '+e.payload+' · log saved','崩'));",
  '',
  '  // Real download/verify progress from the launch pipeline.',
  "  listen('launch://stage', e=>{",
  '    const s=e.payload;',
  '    if(window.__onStage)window.__onStage(s);',
  "    CON.push('INFO','Launcher', s.label + (s.detail ? ' — ' + s.detail : ''));",
  '  });',
  "  listen('launch://mod-problem', e=>{",
  '    const p=e.payload;',
  "    CON.push('WARN','ModLoader','[' + p.kind + '] ' + p.detail);",
  "    toast(p.detail,'警');",
  '  });',
  '',
  '  window.__nativeLaunch = async (opts)=>{',
  '    opts = opts || {};',
  "    const pid = await invoke('launch_game',{",
  "      instance: opts.instance || 'main',",
  "      version:  opts.version  || '1.20.4',",
  "      player:   opts.player   || 'Player',",
  "      uuid:     opts.uuid     || '',",
  "      token:    opts.token    || '',",
  '      memory:   opts.memory   || 4096,',
  '      java:     opts.java     || null',
  '    });',
  '    CON.attach(pid);',
  '    return pid;',
  '  };',
  "  window.__nativeKill   = ()=>invoke('kill_game');",
  "  window.__scanMods     = (instance,loader)=>invoke('scan_mods',{instance,loader});",
  "  window.__installMod   = (instance,source)=>invoke('install_mod',{instance,source});",
  "  window.__toggleMod    = (path,enabled)=>invoke('toggle_mod',{path,enabled});",
  "  window.__deleteMod    = (path)=>invoke('delete_mod',{path});",
  "  window.__listVersions = ()=>invoke('list_versions');",
  '',
  '  // Accounts: the vault is the only source of identity. No seed data.',
  "  window.__beginLogin     = ()=>invoke('begin_login');",
  "  window.__beginDemo      = (nickname)=>invoke('begin_demo',{nickname});",
  "  window.__currentAccount = ()=>invoke('current_account');",
  "  window.__logout         = (uuid)=>invoke('logout',{uuid});",
  "  window.__setDemo        = (on)=>invoke('set_demo',{on});",
  "  window.__gameRunning    = ()=>invoke('game_running');",
  "  window.__openLogDir     = ()=>invoke('open_log_dir');",
  '',
  '  // Populate the account UI from the real vault once the bridge exists.',
  '  if(typeof acctRefresh===\'function\') acctRefresh();',
  '  if(typeof mmSyncNative===\'function\') mmSyncNative();',
  '})();',
  '',
].join('\n');

const anchor = "document.getElementById('palOpen')?.addEventListener('click',()=>PAL.show());";
if (!src.includes(anchor)) {
  console.error('sync failed: anchor not found in prototype');
  process.exit(1);
}

const out = src.replace(anchor, BRIDGE + '\n' + anchor);
mkdirSync(resolve(root, 'launcher/dist'), { recursive: true });
writeFileSync(resolve(root, 'launcher/dist/index.html'), out);
console.log(`  synced prototype -> launcher/dist/index.html (${(out.length / 1024).toFixed(0)} KB)`);

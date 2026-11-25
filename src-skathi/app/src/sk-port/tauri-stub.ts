// Stands in for Tauri's IPC on Skathi. mpv (`plugin:libmpv|*`), the open panel and the files from
// the command line go to the native `mpv` module; every other command answers a neutral value.
// Every command is counted, so `window.__skPortCalls` lists the whole Tauri boundary the
// interface touched — the list of what is still a stub.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import * as nativeMpv from 'native:mpv';
import { fake } from './mpv-fake';
// On a stock workload (no embedder) `native:mpv` is a stand-in that throws: use the JS fake instead.
const mpv = (typeof (globalThis as Record<string, unknown>).__sk_native_module === 'function'
  ? nativeMpv
  : fake) as typeof nativeMpv;
import * as nativeCast from 'native:cast';
// Only a host built with the skathi crate has the module (a stock workload's stand-in throws).
const hasCast = typeof (globalThis as Record<string, unknown>).__sk_native_module === 'function';
import { appFiles, appWindow } from '@skathi-native/runtime';
import { installFileDrop } from './file-drop';
// A leaf (no imports): the chrome reads it, so importing the chrome itself here would be a cycle.
import { windowTransition } from '../../../../src/lib/window-transition';

// mpv's events, as the plugin delivers them: one Tauri event per mpv event, on `mpv-event-<window>`.
mpv.on('event', (payload) => void emit('mpv-event-main', payload));
// Now Playing's commands (media keys, Control Center, AirPods), as the player's Rust emits them.
mpv.on('remote', (command) => void emit('frameplayer://remote', command));
// macOS: the green button and AppKit's menu item (fullscreen.rs), handed to the page's own command so
// every way into fullscreen takes one path.
mpv.on('fullscreen', () => void emit('frameplayer://menu', 'fullscreen'));
// macOS: a live resize by the pointer (live_resize.rs), which hides the bars for its length.
mpv.on('resizing', (p) => void emit('frameplayer://window-resizing', !!p?.on));
// On a layer route the picture is a native layer under the page, so the window has to be
// see-through wherever the page paints nothing (the Tauri window the player was written for is).
try {
  if (mpv.layerRoute()) (appWindow as unknown as { backdrop: string }).backdrop = 'transparent';
} catch {
  /* a host without the call */
}
// macOS: the traffic lights where version 1 has them. Its window grows the title bar to 66 pt with
// an empty toolbar and lets AppKit place them; Skathi centres them in our 48 pt bar at (12, 17),
// 7 pt left of version 1's and 2 pt lower. (19, 19) puts the three discs on version 1's pixels
// (measured, window captures of both). Skathi's setter takes the close
// button's origin in its title-bar container, keeps it across hide/show and fullscreen, and moves
// the tracking areas with the buttons.
if (!navigator.userAgent.includes('Windows')) {
  try {
    (appWindow as unknown as { setTrafficLightPosition(p: { x: number; y: number }): void }).setTrafficLightPosition({ x: 19, y: 19 });
  } catch {
    /* a host without the call */
  }
}
// Windows: the overlay element (subtitles and mpv's OSD, Root.svelte) is the bottom third of the
// video until the host sees text above that; the host then follows whatever size the element has.
if (navigator.userAgent.includes('Windows')) document.documentElement.toggleAttribute('data-sk-overlay-band', true);
// Windows: rules for its text rasterizer only (Overrides.svelte, `font-dilation`).
if (navigator.userAgent.includes('Windows')) document.documentElement.toggleAttribute('data-sk-windows', true);
mpv.on('overlay', (payload) => document.documentElement.toggleAttribute('data-sk-overlay-full', !!payload?.full));
// macOS, FPUI_OVL_BAND=auto (off by default): the overlay element becomes a bottom band of the
// fraction of the window's height the host asks for, following what mpv drew.
mpv.on('band', (p) => {
  const f = Number(p?.f ?? 1);
  const o = document.querySelector('sk-surface[name=overlay]') as HTMLElement | null;
  if (!o) return;
  if (f >= 1) {
    o.style.removeProperty('top');
    o.style.removeProperty('bottom');
    o.style.removeProperty('height');
    return;
  }
  o.style.setProperty('top', 'auto');
  o.style.setProperty('bottom', '0px');
  o.style.setProperty('height', `${(f * 100).toFixed(1)}%`);
});

const calls: Record<string, number> = {};
(globalThis as Record<string, unknown>).__skPortCalls = calls;

const LISTS = /(_list|_devices|_routes|_entries|_titles|_interfaces|_fonts_list|_trending|_search|_releases|track_numbers|_durations|_dirs|paths_exist)$/;

/** A native failure as Tauri hands it over: the rejection is the message itself, not an Error. */
function tauriError(e: unknown): never {
  throw e instanceof Error ? e.message : String(e);
}

type Args = Record<string, unknown> | undefined;

function libmpv(cmd: string, args: Args): unknown {
  const a = (args ?? {}) as {
    name?: string;
    value?: mpv.Value;
    args?: mpv.Value[];
    format?: string;
    mpvConfig?: { initialOptions?: Record<string, mpv.Value>; observedProperties?: Record<string, string> };
  };
  try {
    switch (cmd.slice('plugin:libmpv|'.length)) {
      case 'init':
        mpv.init(Object.entries(a.mpvConfig?.initialOptions ?? {}), Object.entries(a.mpvConfig?.observedProperties ?? {}));
        return 'main';
      case 'destroy':
        mpv.destroy();
        return null;
      case 'command':
        mpv.command(a.name ?? '', a.args ?? []);
        return null;
      case 'set_property':
        mpv.setProperty(a.name ?? '', a.value ?? null);
        return null;
      case 'get_property':
        return mpv.getProperty(a.name ?? '', a.format ?? 'string');
      case 'set_video_margin_ratio':
        return null;
    }
  } catch (e) {
    tauriError(e);
  }
  return null;
}

async function dialogOpen(args: Args): Promise<string | string[] | null> {
  const o = ((args ?? {}) as { options?: { multiple?: boolean; directory?: boolean } }).options ?? {};
  const paths = await mpv.openDialog(!!o.multiple, !!o.directory);
  if (o.multiple) return paths.length ? paths : null;
  return paths[0] ?? null;
}

/**
 * Tauri's path plugin, as far as the interface uses it: the resource directory and joining. Windows
 * paths here: either separator splits, the platform's own joins (mpv and the Rust side take both).
 */
const RESOURCE = 11; // BaseDirectory.Resource
const SEP = navigator.userAgent.includes('Windows') ? '\\' : '/';
/** The host's data directory (settings files, caches, screenshots). */
const dataDir = () => mpv.dataDir();
function path(cmd: string, args: Args): unknown {
  const a = (args ?? {}) as { directory?: number; path?: string; paths?: string[] };
  const joined = (parts: string[]) => {
    const s = parts.filter((p) => p !== '').join(SEP);
    // A doubled separator collapses, except the two that begin a UNC path.
    return s.slice(0, 2) + s.slice(2).replace(/[\\/]{2,}/g, SEP);
  };
  const cut = (p: string) => Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  switch (cmd) {
    case 'resolve_directory': {
      const base = a.directory === RESOURCE ? mpv.resourceDir() : dataDir();
      return a.path ? joined([base, a.path]) : base;
    }
    case 'join':
    case 'resolve':
      return joined(a.paths ?? []);
    case 'normalize':
      return a.path ?? '';
    case 'dirname': {
      const p = a.path ?? '', i = cut(p);
      return i > 0 ? p.slice(0, i) : SEP;
    }
    case 'basename': {
      const p = a.path ?? '';
      return p.slice(cut(p) + 1);
    }
    case 'is_absolute':
      return /^([a-zA-Z]:[\\/]|[\\/])/.test(a.path ?? '');
  }
  return dataDir();
}

/** Tauri's `{ Logical | Physical: { … } }`, or the `Size`/`Position` object before serialization, in CSS px. */
function logical(value: unknown): Record<string, number> {
  const v = value as { size?: { type: string } & Record<string, number>; position?: { type: string } & Record<string, number> } & Record<string, Record<string, number>>;
  const inner = v?.size ?? v?.position;
  if (inner) {
    const k = inner.type === 'Physical' ? 1 / appWindow.scaleFactor : 1;
    return Object.fromEntries(Object.entries(inner).filter(([, n]) => typeof n === 'number').map(([key, n]) => [key, (n as number) * k]));
  }
  if (v?.Physical) return Object.fromEntries(Object.entries(v.Physical).map(([key, n]) => [key, n / appWindow.scaleFactor]));
  return v?.Logical ?? {};
}
const physical = (r: { width: number; height: number }) => ({ width: Math.round(r.width * appWindow.scaleFactor), height: Math.round(r.height * appWindow.scaleFactor) });
const physicalAt = (r: { x: number; y: number }) => ({ x: Math.round(r.x * appWindow.scaleFactor), y: Math.round(r.y * appWindow.scaleFactor) });
function monitor() {
  const k = appWindow.scaleFactor, sc = window.screen as Screen & { availLeft?: number; availTop?: number };
  return {
    name: 'main',
    size: { width: Math.round(sc.width * k), height: Math.round(sc.height * k) },
    position: { x: 0, y: 0 },
    workArea: { size: { width: Math.round(sc.availWidth * k), height: Math.round(sc.availHeight * k) }, position: { x: Math.round((sc.availLeft ?? 0) * k), y: Math.round((sc.availTop ?? 0) * k) } },
    scaleFactor: k,
  };
}

/**
 * Fullscreen. Through black in both directions when `fsAnimMs` is 0 (argon-chat/skathi#35, since Skathi b5fe755: what the
 * player's native mask did; on Windows Skathi's `fade` is a plain switch and the page's own veil is the
 * black).
 *
 * Windows `sklayer`, the default (`FPUI_FS_ANIM=0` turns it off): no black.
 * `html[data-fs-anim]` hides the page's veil and the interface for the transition (Root.svelte); the
 * host animates the video's visual between its old box and the new one while the swap chain keeps its
 * size, the picture keeping its shape, and resizes the chain once at the end (windows/mpv.rs `FsAnim`).
 * On the way out the window stays fullscreen while the picture shrinks back, and is restored when the
 * animation is over.
 */
let fsAnimMs = 0;
try {
  fsAnimMs = (mpv as unknown as { fsAnimMs?: () => number }).fsAnimMs?.() ?? 0;
} catch {
  /* a host without the call */
}
// The page's veil comes down before `set_fullscreen` is called (chrome.svelte.ts `presentVeil`), so
// under the flag it is away for good, not per transition (Root.svelte).
if (fsAnimMs > 0) document.documentElement.toggleAttribute('data-fs-anim-on', true);
// Both ways and on both systems, the bars go before the transition and stay away after it until the
// viewer moves the pointer or presses a key that raises them (chrome.svelte.ts `hushForTransition`).
// Pointer reports during the transition plus 300 ms are not activity: the window changing under a
// resting pointer sends one. Under the animation `begin` takes the whole interface off at once
// (`data-fs-anim`, Root.svelte) rather than over the bars' 250 ms fade; `setFullscreen` puts it back
// once the bars are already idle-hidden under it. A transition that never reaches `setFullscreen`
// gives it back after 2 s.
let beginTimer: ReturnType<typeof setTimeout> | undefined;
windowTransition.hushChrome = true;
windowTransition.requests = true;
windowTransition.graceMs = (fsAnimMs > 0 ? fsAnimMs : 300) + 300;
if (fsAnimMs > 0) {
  windowTransition.begin = () => {
    const root = document.documentElement;
    root.toggleAttribute('data-fs-anim', true);
    clearTimeout(beginTimer);
    beginTimer = setTimeout(() => root.toggleAttribute('data-fs-anim', false), 2000);
  };
}
/** Two frames: the interface as `begin` left it is on screen before the window or the picture moves. */
const presented = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
const IS_WINDOWS = navigator.userAgent.includes('Windows');

/**
 * The host's fullscreen queue, when it has one. The page asks for the state it wants, at once (chrome.svelte.ts,
 * `windowTransition.queued`); the host owns the transition, runs one at a time, goes on to the last request when
 * more came meanwhile (an even number of toggles cancels out), and reports through `is_fullscreen` the state it is
 * heading to while anything runs or waits (`fsTarget`), the window's own state otherwise. A host without these
 * calls keeps the old behaviour: no queue, the page's veil and the real state.
 */
type FsHost = {
  fsQueued?: () => boolean;
  fsTarget?: () => boolean | null;
  fsRequest?: (on: boolean, actual: boolean) => 'enter' | 'exit' | 'queued' | 'none';
  fsNext?: (actual: boolean) => 'enter' | 'exit' | 'none';
  fsWanted?: (on: boolean) => boolean;
  fsAnimBusy?: () => boolean;
};
const fsHost = mpv as unknown as FsHost;
const hostCall = <T>(f: () => T, fallback: T): T => {
  try {
    return f();
  } catch {
    return fallback;
  }
};
windowTransition.queued = hostCall(() => fsHost.fsQueued?.() ?? false, false);
const fsTarget = (): boolean | null => hostCall(() => fsHost.fsTarget?.() ?? null, null);

// Windows: the host decides (windows/mpv.rs `FsQueue`), the stub carries each step out.
let fsRun: Promise<null> | null = null;
let doneTimer: ReturnType<typeof setTimeout> | undefined;
function fsDone() {
  clearTimeout(doneTimer);
  doneTimer = setTimeout(() => document.documentElement.toggleAttribute('data-fs-anim', false), 150);
}
async function fsStep(on: boolean) {
  const root = document.documentElement;
  clearTimeout(beginTimer);
  clearTimeout(doneTimer);
  root.toggleAttribute('data-fs-anim', true);
  // The same two frames as before, so that no frame of the moving picture carries the interface at its old
  // place (otherwise 40 ms of the bars at the fullscreen layout on the way out, measured).
  await presented();
  if (!fsHost.fsWanted!(on)) return; // a toggle came back before anything moved
  if (on) {
    mpv.fsAnimEnter(fsAnimMs);
    appWindow.setFullscreen(true);
    await new Promise((r) => setTimeout(r, fsAnimMs));
  } else if (mpv.fsAnimExit(fsAnimMs)) {
    await new Promise((r) => setTimeout(r, fsAnimMs));
    appWindow.setFullscreen(false);
  } else {
    appWindow.setFullscreen(false);
  }
  // The interface's mask comes off as it did with one transition; the next step waits for the host as well.
  fsDone();
  const until = performance.now() + 1000;
  while (hostCall(() => fsHost.fsAnimBusy?.() ?? false, false) && performance.now() < until) {
    await new Promise((r) => requestAnimationFrame(r));
  }
}
async function fsSteps(first: 'enter' | 'exit'): Promise<null> {
  let step: string = first;
  try {
    while (step === 'enter' || step === 'exit') {
      await fsStep(step === 'enter');
      step = fsHost.fsNext!(appWindow.isFullscreen());
    }
  } catch (e) {
    // The host's queue must not be left running: put it back to idle on the window's real state.
    console.warn('fullscreen step failed:', e);
    const actual = appWindow.isFullscreen();
    hostCall(() => (fsHost.fsRequest?.(actual, actual), fsHost.fsNext?.(actual)), undefined);
  } finally {
    fsRun = null;
  }
  fsDone();
  return null;
}
function setFullscreenQueued(on: boolean): Promise<null> | null {
  const step = fsHost.fsRequest!(on, appWindow.isFullscreen());
  if (step === 'queued') return fsRun;
  if (step === 'none') {
    fsDone();
    return null;
  }
  fsRun = fsSteps(step);
  return fsRun;
}

/**
 * macOS: the page's latest request until the host has it. The page asks in the same tick as the key or click,
 * but the request reaches the host two frames later (below); in between a resize makes the page re-read
 * `is_fullscreen`, and the host's target alone would take the request back.
 */
let macPending: boolean | null = null;
function macTarget(): boolean | null {
  if (macPending === null) return null;
  if (fsTarget() === macPending || (fsTarget() === null && appWindow.isFullscreen() === macPending)) macPending = null;
  return macPending;
}

function setFullscreen(on: boolean): Promise<null> | null {
  if (fsAnimMs <= 0) {
    appWindow.setFullscreen(on, { transition: 'fade' });
    return null;
  }
  const root = document.documentElement;
  clearTimeout(beginTimer);
  root.toggleAttribute('data-fs-anim', true);
  const done = () => setTimeout(() => root.toggleAttribute('data-fs-anim', false), 150);
  // macOS: AppKit animates snapshots of the window, taken when the transition starts, so the hidden
  // interface has to be on screen first (two frames, as version 1's veil waits); otherwise the bars
  // ride along in the snapshot and cross-fade against the picture (measured). Both ways;
  // the interface comes back `fsAnimMs` later, after AppKit's ~550 ms.
  if (!IS_WINDOWS) {
    macPending = on;
    // The state, not a toggle: the host queues a request that comes while a transition runs and applies
    // the latest one after it (fullscreen.m `fpui_fs_want`); Skathi's `setFullscreen` compares with the
    // window's style mask, already the target's mid-transition, and quick requests were lost.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        mpv.fsWant(on);
        setTimeout(done, fsAnimMs);
      }),
    );
    return null;
  }
  if (fsHost.fsRequest && fsHost.fsNext && fsHost.fsWanted) return setFullscreenQueued(on);
  // A Windows host without the queue: one transition at a time is not enforced.
  return presented().then(() => {
    if (on) {
      mpv.fsAnimEnter(fsAnimMs);
      appWindow.setFullscreen(true);
      setTimeout(done, fsAnimMs);
      return null;
    }
    if (!mpv.fsAnimExit(fsAnimMs)) {
      appWindow.setFullscreen(false);
      done();
      return null;
    }
    return new Promise<null>((resolve) =>
      setTimeout(() => {
        appWindow.setFullscreen(false);
        done();
        resolve(null);
      }, fsAnimMs),
    );
  });
}

/** `plugin:window|*` on Skathi's `appWindow`. Sizes and positions cross as Tauri's physical pixels. */
function windowCmd(cmd: string, args: Args): unknown {
  const a = (args ?? {}) as { value?: unknown };
  switch (cmd) {
    case 'scale_factor': return appWindow.scaleFactor;
    // On macOS `getContentBounds()` is the whole frame, title bar included; the viewport is the client.
    case 'inner_size': return physical({ width: window.innerWidth, height: window.innerHeight });
    case 'outer_size': return physical(appWindow.getBounds());
    case 'inner_position': { const b = appWindow.getBounds(); return physicalAt({ x: b.x, y: b.y + b.height - window.innerHeight }); }
    case 'outer_position': return physicalAt(appWindow.getBounds());
    // While the host's queue runs or waits, the state it is heading to (`fsTarget`); on macOS before that the
    // page's own request on its way to the host (`macTarget`).
    case 'is_fullscreen': return (IS_WINDOWS ? null : macTarget()) ?? fsTarget() ?? appWindow.isFullscreen();
    case 'is_maximized': return appWindow.isMaximized();
    case 'is_minimized': return appWindow.isMinimized();
    case 'is_focused': return appWindow.isFocused();
    case 'is_visible': return appWindow.isVisible();
    case 'is_always_on_top': return appWindow.alwaysOnTop;
    case 'is_decorated': case 'is_resizable': case 'is_maximizable': case 'is_minimizable': case 'is_closable': case 'is_enabled': return true;
    case 'title': return appWindow.title;
    case 'set_title': appWindow.title = String(a.value ?? ''); return null;
    case 'maximize': appWindow.maximize(); return null;
    case 'unmaximize': appWindow.unmaximize(); return null;
    case 'toggle_maximize': appWindow.toggleMaximize(); return null;
    case 'minimize': appWindow.minimize(); return null;
    case 'unminimize': appWindow.restore(); return null;
    case 'show': case 'set_focus': if (!mpv.hidden()) appWindow.show(); return null;
    case 'hide': appWindow.hide(); return null;
    case 'close': case 'destroy': appWindow.close(); return null;
    case 'center': appWindow.center(); return null;
    case 'set_always_on_top': appWindow.alwaysOnTop = !!a.value; return null;
    // Through black in both directions (argon-chat/skathi#35, since Skathi b5fe755): what the player's native mask did.
    case 'set_fullscreen': return setFullscreen(!!a.value);
    case 'start_dragging': appWindow.startDragging(); return null;
    // Skathi's startResizeDragging() only logs "not implemented", so the page's 5 px strips inside
    // the edge showed the arrow and swallowed the press. The host starts the system's sizing loop.
    case 'start_resize_dragging': mpv.startResize(String(a.value ?? '')); return null;
    case 'set_size': { const v = logical(a.value); appWindow.setContentBounds({ width: v.width, height: v.height }); return null; }
    case 'set_position': { const v = logical(a.value); appWindow.setBounds({ x: v.x, y: v.y }); return null; }
    // Run-time limits since Skathi 6a9338be (argon-chat/skathi#35), on the content area in CSS px; null lifts them.
    case 'set_min_size': case 'set_max_size': {
      const v = a.value ? logical(a.value) : {};
      const size = v.width && v.height ? { width: v.width, height: v.height } : null;
      if (cmd === 'set_min_size') appWindow.setMinSize(size);
      else appWindow.setMaxSize(size);
      return null;
    }
    case 'set_progress_bar': appWindow.setProgressBar(a.value); return null;
    case 'current_monitor': case 'primary_monitor': case 'monitor_from_point': return monitor();
    case 'available_monitors': return [monitor()];
    case 'cursor_position': return { x: 0, y: 0 };
    // One window. The player's Windows fullscreen covers the switch with a second, black window
    // ("veil"); there is none here, so `getByLabel('veil')` must find nothing rather than fail.
    case 'get_all_windows': return [{ label: 'main' }];
  }
  return null;
}

// Window events, as Tauri's window listeners expect them (the mock ignores the target).
appWindow.addEventListener('resize', () => void emit('tauri://resize', physical({ width: window.innerWidth, height: window.innerHeight })));
appWindow.addEventListener('move', () => void emit('tauri://move', physicalAt(appWindow.getBounds())));
appWindow.addEventListener('focus', () => void emit('tauri://focus', null));
appWindow.addEventListener('blur', () => void emit('tauri://blur', null));
// Tauri asks the page, which closes with `destroy()` unless a handler prevented it.
appWindow.addEventListener('close-requested', (e: Event) => { e.preventDefault(); void emit('tauri://close-requested', null); });

/**
 * Windows: a wheel notch in the page's pixels, as WebView2 counts them. gpui hands Skathi a notch as
 * the system's lines per notch (3 by default), and Skathi makes each line 16 px (`pixel_delta(16px)`),
 * so a notch read 48 px (measured); Chromium counts a line 100/3 px, a notch 100 px, and the player's
 * wheel rules are written for that (`PX_PER_VOLUME_UNIT` = 20: 5 a notch, 2–3 here). The event says
 * nothing of lines (`deltaMode` is always 0), so a line is recognised by its value: a whole number of
 * 16 px. A precision touchpad's scroll comes through DirectManipulation as pixels and a pinch as
 * −100·ln(scale), both fractional, and stay as they are; a high-resolution wheel's part of a notch
 * is not rescaled. The deltas are rewritten before any of the page's listeners; Skathi's own
 * scrolling does not read them. KNOWN-ISSUES.md, "The wheel notch is 48 px".
 */
if (navigator.userAgent.includes('Windows')) {
  const LINE_PX = 16;
  const CHROMIUM_LINE_PX = 100 / 3;
  const lines = (d: number) => d !== 0 && Number.isInteger(d / LINE_PX);
  addEventListener('wheel', (e: WheelEvent) => {
    if (e.deltaMode !== 0 || !(lines(e.deltaX) || lines(e.deltaY))) return;
    for (const k of ['deltaX', 'deltaY'] as const) {
      Object.defineProperty(e, k, { value: (e[k] / LINE_PX) * CHROMIUM_LINE_PX, configurable: true });
    }
  }, { capture: true, passive: true });
}

// Files dropped on the window (Finder, Explorer) reach the player's `onDragDropEvent` as Tauri's
// `tauri://drag-*` events: Skathi delivers them as DOM drag events (file-drop.ts).
installFileDrop(document, emit, (f) => appFiles.getPathForFile(f), () => appWindow.scaleFactor);

/** The last pointer position the page saw, for `window_buttons` (CSS px, viewport). */
const pointer = { x: -1, y: -1 };
addEventListener('pointermove', (e) => { pointer.x = e.clientX; pointer.y = e.clientY; }, { capture: true, passive: true });

/**
 * The pinch, as the player's native monitor split it (macos_chrome.rs): a plain pinch resizes the
 * window when `window_pinch_mode` says so; ⌥+pinch, and a pinch the player keeps for the zoom, go on
 * to its `wheel` handler. Skathi delivers a trackpad pinch as a cancelable `wheel` with `ctrlKey`, the
 * modifiers kept, `GestureEvent`s first; a physical Ctrl+wheel sets `ctrlKey` too but has no gesture,
 * so the last gesture event tells the two apart. Scale per event = exp(-deltaY / 100).
 */
const pinch = { resize: false, gestureAt: -1e9 };
for (const t of ['gesturestart', 'gesturechange', 'gestureend']) {
  addEventListener(t, () => { pinch.gestureAt = performance.now(); }, { capture: true, passive: true });
}
addEventListener('wheel', (e: WheelEvent) => {
  if (!e.ctrlKey || e.altKey || !pinch.resize || performance.now() - pinch.gestureAt > 150) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const step = Math.exp(-e.deltaY / 100);
  const b = appWindow.getBounds();
  const cw = window.innerWidth, ch = window.innerHeight;
  // Whole points, and nothing when nothing changes: fractional frames trapped AppKit (memory note).
  const w = Math.round(cw * step), h = Math.round(ch * step);
  if (w === cw && h === ch) return;
  const x = Math.round(b.x - (w - cw) / 2), y = Math.round(b.y - (h - ch) / 2);
  appWindow.setBounds({ x, y, width: b.width + (w - cw), height: b.height + (h - ch) });
}, { capture: true, passive: false });

/**
 * The player's own window commands (lib.rs, window_shape.rs, macos_chrome.rs), on `appWindow` where
 * Skathi has the verb: Skathi 6a9338be and b5fe755 (argon-chat/skathi#35). `undefined` for any other command.
 *
 * - `window_buttons` (the traffic lights with the idle chrome): `setWindowButtonsVisible` with the
 *   player's 150 ms fade. The answer the frontend wants is whether the pointer is on them now, which
 *   it cannot see itself; here it is the last pointer position the page saw, against
 *   `getWindowButtonsRect()` read before the change.
 * - `window_enter_fullscreen`: `setFullscreen(true, { transition: "fade" })`, `true` = handled. The
 *   exit goes through `plugin:window|set_fullscreen`, with the same transition.
 * - `window_fullscreen_mask`: nothing, the fade transition is the mask.
 * - `window_pinch_mode`: kept here; the `wheel` listener below resizes the window on a plain pinch
 *   when the player asks for it, and leaves ⌥+pinch (`altKey`, kept by Skathi) and Ctrl+wheel to the
 *   player's zoom.
 * - The mini player's `window_float_over_fullscreen` → `visibleOnAllWorkspaces`; with
 *   `alwaysOnTop` beside it Skathi moves the content into a nonactivating panel (b5fe755).
 * - `window_video_fade`: an error, so `Curtain.svelte` draws the dark. That is the right path here
 *   rather than a fallback: mpv draws into `<sk-surface>`, a box in the same tree, so a fill over it
 *   lands in the same frame as the window's size, which the player's web view could not do.
 */
function windowOwn(cmd: string, args: Args): unknown {
  const a = (args ?? {}) as Record<string, number | boolean>;
  switch (cmd) {
    case 'window_shape_lock': {
      const w = Number(a.width) || 0, h = Number(a.height) || 0;
      appWindow.setAspectRatio(w > 0 && h > 0 ? w / h : null);
      // Windows: the host decides which side leads on a corner drag (windows/sys.rs `corner_lead`); the call is
      // not there on macOS, where AppKit holds the shape.
      (mpv as unknown as { setAspectHold?: (r: number) => void }).setAspectHold?.(w > 0 && h > 0 ? w / h : 0);
      return null;
    }
    // With `alwaysOnTop`, which the mini player sets beside it, this floats over other apps' fullscreen Spaces.
    case 'window_float_over_fullscreen': appWindow.visibleOnAllWorkspaces = !!a.on; return null;
    case 'window_frame_glide': {
      // The player's frame is the window's, in physical px. Its window has no title bar, and its
      // fallback is `setSize` (the content) plus `setPosition` (the frame's corner), so the target is
      // the content size with this window's chrome added back, as that fallback would land.
      const k = appWindow.scaleFactor, b = appWindow.getBounds();
      // Windows: the window has no title bar (as the player's), and `getBounds()` is larger than
      // the viewport by the invisible resize border, which `setBounds` does not take back: adding
      // it grew the window by 14.67 px on every file (measured). No chrome is added there.
      const mac = !navigator.userAgent.includes('Windows');
      const chromeW = mac ? b.width - window.innerWidth : 0, chromeH = mac ? b.height - window.innerHeight : 0;
      const ms = Number(a.ms) || 0;
      appWindow.setBounds(
        { x: Number(a.x) / k, y: Number(a.y) / k, width: Number(a.width) / k + chromeW, height: Number(a.height) / k + chromeH },
        { animate: ms > 0 ? { duration: ms, easing: 'ease-in-out' } : false },
      );
      // The player's command answers once the window is there; so does this.
      return ms > 0 ? new Promise((done) => setTimeout(() => done(true), ms)) : true;
    }
    case 'window_video_fade': return Promise.reject('no native video view: the curtain draws the dark');
    case 'window_buttons': {
      const r = appWindow.getWindowButtonsRect();
      const on = !!r && pointer.x >= r.x && pointer.x < r.x + r.width && pointer.y >= r.y && pointer.y < r.y + r.height;
      appWindow.setWindowButtonsVisible(!!a.visible, { fade: 150 });
      return on;
    }
    // macOS enters through here (chrome.svelte.ts), so FPUI_FS_ANIM has to as well.
    case 'window_enter_fullscreen': setFullscreen(true); return true;
    case 'window_fullscreen_mask': return null;
    case 'window_pinch_mode': pinch.resize = !!a.resize; return null;
  }
  return undefined;
}

/** The bytes of a `Uint8Array` as an `ArrayBuffer` of their own, which is what Tauri's raw responses are. */
function arrayBuffer(u: Uint8Array): ArrayBuffer {
  return u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength) as ArrayBuffer;
}

/** The player's commands that `native:mpv` carries (src/commands.rs); `undefined` for any other. */
function own(cmd: string, args: Args): unknown {
  const a = (args ?? {}) as Record<string, unknown>;
  const str = (k: string) => String(a[k] ?? '');
  const call = (f: () => unknown) => {
    try {
      const r = f();
      // A command with no result answers null, as Tauri's do, so it does not fall through to a stub below.
      return r instanceof Promise ? r.catch(tauriError) : (r ?? null);
    } catch (e) {
      tauriError(e);
    }
  };
  switch (cmd) {
    case 'folder_entries': return call(() => mpv.folderEntries(str('path')));
    case 'paths_exist': return call(() => mpv.pathsExist((a.paths as string[]) ?? []));
    case 'external_track_dirs': return call(() => mpv.externalTrackDirs(str('path')));
    case 'user_mpv_conf': return call(() => mpv.userMpvConf());
    case 'mpv_conf_set': return call(() => mpv.mpvConfSet(str('key'), a.value == null ? null : String(a.value)));
    case 'third_party_notices': return call(() => mpv.thirdPartyNotices());
    case 'screenshot_dir': return call(() => mpv.screenshotDir());
    case 'screenshot_clip_path': return call(() => mpv.screenshotClipPath());
    case 'screenshot_to_clipboard': return call(() => mpv.screenshotToClipboard(str('path')));
    case 'trash_file': return call(() => mpv.trashFile(str('path')));
    case 'primary_button_down': return call(() => mpv.primaryButtonDown());
    case 'double_click_time': return call(() => mpv.doubleClickTime());
    case 'debug_log_state': return call(() => mpv.debugLogState());
    case 'debug_log_set': return call(() => mpv.debugLogSet(!!a.on));
    case 'subtitle_fonts_dir': return call(() => mpv.subtitleFontsDir());
    case 'subtitle_fonts_list': return call(() => mpv.subtitleFontsList());
    case 'subtitle_font_add': return call(() => mpv.subtitleFontAdd(str('path')));
    case 'subtitle_font_remove': return call(() => mpv.subtitleFontRemove(str('name')));
    case 'now_playing_start': return call(() => mpv.nowPlayingStart());
    case 'now_playing_set': return call(() => mpv.nowPlayingSet(a.info));
    case 'mpv_shader_cache_dir': return call(() => mpv.mpvShaderCacheDir());
    case 'zoom_pan': return call(() => mpv.zoomPan(Number(a.z) || 0, Number(a.x) || 0, Number(a.y) || 0));
    // thumb_service.rs (src/thumbs.rs). Tauri's raw Response arrives as an ArrayBuffer; the module's bytes are a Uint8Array.
    case 'thumb_start': return call(() => mpv.thumbStart(str('path'), Number(a.duration) || 0));
    case 'thumb_get': return call(() => mpv.thumbGet(str('path'), Number(a.pos) || 0, !!a.exact).then(arrayBuffer));
    case 'set_private_paths': return call(() => mpv.setPrivatePaths((a.paths as string[]) ?? [], !!a.all));
    case 'forget_thumbs': return call(() => mpv.forgetThumbs(str('path')));
    case 'forget_thumbs_under': return call(() => mpv.forgetThumbsUnder(str('folder')));
    case 'clear_thumb_cache': return call(() => mpv.clearThumbCache());
    case 'poster_frame': return call(() => mpv.posterFrame(str('path'), Number(a.pos) || 0).then(arrayBuffer));
    case 'poster_saved': return call(() => arrayBuffer(mpv.posterSaved(str('id'))));
    case 'poster_capture': return call(() => mpv.posterCapture(str('id'), str('path'), (a.positions as number[]) ?? []));
    case 'crop_detect': return call(() => mpv.cropDetect(str('path'), (a.positions as number[] | null | undefined) ?? null));
    case 'container_titles': return call(() => mpv.containerTitles((a.paths as string[]) ?? []));
  }
  return undefined;
}

/**
 * Casting: version 1's discovery runs in the host (`native:cast`), so the cast menu lists the
 * televisions Google Cast and DLNA find. Nothing past the list is ported (the connection, the LAN
 * file server, the session), so picking one is refused with a message rather than answered with a
 * neutral value the menu would take for a connection. `undefined` for any other command.
 */
const CAST_NOT_YET = 'Casting to a television is not in this beta yet: the player finds the device but cannot play on it.';
function castCmd(cmd: string, args: Args): unknown {
  const hints = (((args ?? {}) as { hints?: string[] }).hints ?? []).map(String);
  const call = (f: () => unknown) => {
    try {
      return f() ?? null;
    } catch (e) {
      tauriError(e);
    }
  };
  switch (cmd) {
    case 'cast_discover_start': return call(() => nativeCast.castDiscoverStart(hints));
    case 'cast_discover_stop': return call(() => nativeCast.castDiscoverStop());
    case 'cast_devices': return call(() => nativeCast.castDevices());
    case 'dlna_discover_start': return call(() => nativeCast.dlnaDiscoverStart(hints));
    case 'dlna_discover_stop': return call(() => nativeCast.dlnaDiscoverStop());
    case 'dlna_devices': return call(() => nativeCast.dlnaDevices());
    case 'cast_connect': case 'dlna_connect': case 'cast_load': case 'cast_load_url': case 'dlna_load': case 'dlna_load_url':
    case 'cast_serve_torrent': case 'cast_prepare': case 'cast_prepare_tracks': case 'cast_hls_prepare': case 'cast_diagnose':
      return Promise.reject(CAST_NOT_YET);
  }
  return undefined;
}

function answer(cmd: string, args: Args): unknown {
  const mine = own(cmd, args);
  if (mine !== undefined) return mine;
  if (hasCast) {
    const cast = castCmd(cmd, args);
    if (cast !== undefined) return cast;
  }
  const win = windowOwn(cmd, args);
  if (win !== undefined) return win;
  if (cmd.startsWith('plugin:libmpv|')) return libmpv(cmd, args);
  if (cmd === 'plugin:dialog|open') return dialogOpen(args);
  if (cmd.startsWith('plugin:path|')) return path(cmd.slice('plugin:path|'.length), args);
  if (cmd === 'plugin:app|version') return '1.49.0';
  if (cmd === 'plugin:app|name') return 'Frame Player';
  if (cmd === 'plugin:app|tauri_version') return '2.0.0';
  if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:webview|')) return windowCmd(cmd.slice(cmd.indexOf('|') + 1), args);
  if (LISTS.test(cmd)) return [];
  if (cmd === 'take_pending_files') return mpv.takePendingFiles();
  if (cmd === 'hdr_status') return { supported: false, enabled: false };
  if (cmd === 'shell_handler_state') return false;
  if (cmd === 'torrent_dir') return [dataDir() + SEP + 'torrents', true];
  if (cmd === 'cast_cache_size') return 0;
  if (cmd === 'subs_account') return null;
  if (cmd === 'webview_version') return 'skathi';
  if (cmd === 'portable_state') return { portable: false };
  // Paths mpv would be handed (log file, shader cache, fonts): none, so mpv is not pointed at
  // a directory that does not exist.
  if (cmd === 'diag_mpv_log_path') return null;
  if (cmd === 'mpv_wid') return null;
  if (/_dir$|_path$/.test(cmd)) return dataDir();
  return null;
}

mockWindows('main');
mockIPC(
  (cmd, args) => {
    calls[cmd] = (calls[cmd] ?? 0) + 1;
    return answer(cmd, args as Args);
  },
  { shouldMockEvents: true },
);

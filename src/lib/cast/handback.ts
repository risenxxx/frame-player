/** The end of a session: the poll stops, the television lets go and mpv takes over at its position. */

import { invoke } from '@tauri-apps/api/core';
import { command, setProperty } from 'tauri-plugin-libmpv-api';

import { frameStepBeforeSeek } from '../frame-step.svelte';
import { showOsd } from '../osd.svelte';
import { player } from '../player.svelte';
import { suspendThumbs } from '../thumbs.svelte';
import { t } from '../i18n.svelte';
import { transportCommand } from './controls';
import { castCacheCapGb } from './prefs';
import { castRecord, noteSession } from './record';
import { cast, session } from './state.svelte';

export function stopPoll() {
  if (session.pollTimer !== null) {
    clearInterval(session.pollTimer);
    session.pollTimer = null;
  }
}

/**
 * End the session and hand playback back to mpv.
 *
 * Every path through here raises a popup — this is what closes the sticky
 * "connecting" one on failures, per the OSD contract.
 */
export async function endCast(opts: {
  osd: string;
  resumeLocal: boolean;
  resumePaused?: boolean;
}): Promise<void> {
  stopPoll();
  suspendThumbs(false);
  // A session that failed before its first poll still gets a record.
  if (!castRecord || castRecord.endedAt !== null) noteSession({ state: cast.state, error: null, fetches: 0 });
  noteSession(null, opts.osd);
  const wasActive = cast.active;
  cast.active = false;
  cast.state = 'idle';
  cast.deviceName = null;
  cast.kind = null;
  // A prepare still running dies with the session; harmless when none is.
  void invoke('cast_prepare_cancel').catch(() => {});
  const lastTime = await invoke<number>(transportCommand('disconnect')).catch(() => 0);
  session.airplayLive = false;
  // The prepared copy of a private file must not outlive its session, and
  // with the cache set to "keep nothing" neither does anyone else's; a public
  // one otherwise stays cached so re-casting it is instant. (HLS sessions are
  // cleaned Rust-side with the server.)
  if (session.preparedPath && (session.preparedHidden || castCacheCapGb() === 0)) {
    void invoke('cast_forget_prepared', { path: session.preparedPath }).catch(() => {});
  }
  session.preparedPath = null;
  session.preparedHidden = false;
  session.castSrcPath = null;
  session.sessionHls = null;
  session.dlnaSubPath = null;
  session.airplayHls = null;
  cast.airplayTracks = false;
  cast.notice = null;
  session.airplayReceiver = null;
  session.currentDevice = null;
  cast.transport = 'cast';
  showOsd(opts.osd);
  if (!wasActive || !opts.resumeLocal || !player.hasFile) return;

  const target = lastTime > 0 ? lastTime : cast.time;
  if (target > 0 && Number.isFinite(target)) {
    // The frame stepper may have left mpv decoding backward when the cast began.
    await frameStepBeforeSeek();
    await command('seek', [target, 'absolute']).catch(() => {});
  }
  const unpause = session.resumeUnpaused && !opts.resumePaused;
  if (unpause) {
    await setProperty('pause', false).catch(() => {});
  }
}

/** The user's own "continue here" action. */
export async function disconnectCast(): Promise<void> {
  await endCast({ osd: t('cast.stopped'), resumeLocal: true });
}

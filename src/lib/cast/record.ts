/** What an issue report says about the current or the last session. */

import { cast, session, type CastStatusMsg, type CastVerdict, type TvDevice, type Transport } from './state.svelte';

/**
 * The current or the last session, as an issue report describes it: the
 * transport, the device by its **model** — what the device announces itself
 * as (`md` for Cast, `modelName` for DLNA, `model` for AirPlay), which the
 * rules record behaviour by — and never by the name somebody gave it, what it
 * said it can do, and the last state it reported. Kept after the session ends:
 * a report is usually collected right after something went wrong.
 */
export interface CastRecord {
  transport: Transport;
  model: string;
  /// Which protocols the row had, and what each said it accepts.
  capabilities: string;
  container: string | null;
  verdict: string | null;
  state: string;
  error: string | null;
  fetches: number;
  startedAt: number;
  endedAt: number | null;
  /// What the session ended with, as the viewer was told.
  ending: string | null;
}

export let castRecord: CastRecord | null = null;

export function lastCastSession(): CastRecord | null {
  return castRecord;
}

function deviceCapabilities(device: TvDevice | null): string {
  if (!device) return '';
  const parts: string[] = [];
  if (device.airplay) parts.push('AirPlay (the system route)');
  if (device.cast) {
    const video = device.cast.video;
    parts.push(`Cast (video out: ${video === undefined || video === null ? 'not announced' : video ? 'yes' : 'no'})`);
  }
  if (device.dlna) {
    const video = device.dlna.mimes.filter((m) => m.startsWith('video/'));
    parts.push(
      `DLNA (${device.dlna.mimes.length} formats${video.length ? `; video: ${[...new Set(video)].join(' ')}` : ''}${device.dlna.rendering_url ? '; volume control' : ''})`,
    );
  }
  return parts.join(' + ');
}

function verdictText(v: CastVerdict | null | undefined): string | null {
  if (!v) return null;
  if (v.kind === 'refuse') return `refuse (${v.reason})`;
  return `${v.kind}${v.transcodeAudio ? ' + audio transcode' : ''}`;
}

export function noteSession(status: Pick<CastStatusMsg, 'state' | 'error' | 'fetches' | 'model'> | null, ending?: string) {
  if (!castRecord || castRecord.endedAt !== null) {
    if (!status) return;
    castRecord = {
      transport: cast.transport,
      model: '',
      capabilities: '',
      container: null,
      verdict: null,
      state: status.state,
      error: null,
      fetches: 0,
      startedAt: Date.now(),
      endedAt: null,
      ending: null,
    };
  }
  const r = castRecord;
  r.transport = cast.transport;
  if (status?.model) r.model = status.model;
  else if (session.currentDevice && !session.currentDevice.airplay) r.model = session.currentDevice.model;
  if (session.currentDevice) r.capabilities = deviceCapabilities(session.currentDevice);
  if (cast.plan) {
    r.container = cast.plan.container;
    r.verdict = verdictText(r.transport === 'airplay' ? cast.plan.airplayVerdict : cast.plan.verdict);
  }
  if (status) {
    r.state = status.state;
    r.error = status.error;
    r.fetches = status.fetches;
  }
  if (ending !== undefined) {
    r.endedAt = Date.now();
    r.ending = ending;
  }
}

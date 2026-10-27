/**
 * The shared state of casting: the types every part of it speaks, the
 * reactive `cast` mirror the components and `playback` read, and the session's
 * own bookkeeping — the plain variables more than one of the modules beside
 * this one writes.
 */

import type { PictureKind } from '../airplay-choice';

export interface CastDevice {
  id: string;
  name: string;
  model: string;
  ip: string;
  port: number;
  /// The announcement's capability bit for video out; null when the device
  /// was found by the unicast sweep, which asks for nothing but a name.
  video?: boolean | null;
}

export interface DlnaDevice {
  id: string;
  name: string;
  model: string;
  ip: string;
  control_url: string;
  rendering_url: string | null;
  /// What the renderer said it accepts (`ConnectionManager::GetProtocolInfo`).
  mimes: string[];
}

/// `airplay` is macOS only and is not a half of any row: the system chooses the
/// receiver from its own list, so the session has no device of ours behind it
/// (docs/airplay.md).
export type Transport = 'cast' | 'dlna' | 'airplay';

/**
 * One television, however many ways there are to reach it.
 *
 * The same set answers on two protocols and **cannot be joined by identifier**
 * — measured on the LG, its Cast id and its renderer's UDN are different UUIDs
 * — so the merge key is the address, which both discoveries report. The
 * remembered profile is keyed differently on purpose: an address is a DHCP
 * lease and would lose the setting silently, so a profile is filed under the
 * most stable id the device has and matched by name+model as a fallback.
 */
export interface TvDevice {
  key: string;
  name: string;
  model: string;
  ip: string;
  cast?: CastDevice;
  dlna?: DlnaDevice;
  /// The AirPlay row's stand-in: the system's list chooses the television, so
  /// there is nothing here but the fact that this is that row.
  airplay?: true;
}

/// The device a session over AirPlay belongs to. The queue follows "the
/// device", and for this transport the device is whatever the system's route
/// is — which outlives a change of file.
export const AIRPLAY_DEVICE: TvDevice = { key: 'airplay', name: 'AirPlay', model: '', ip: '', airplay: true };

/// Where the AirPlay row is on the page, so the system's list opens beside it.
export interface Anchor {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CastStatusMsg {
  state: string;
  error: string | null;
  time: number;
  duration: number;
  volume: number;
  muted: boolean;
  volume_known: boolean;
  volume_fixed: boolean;
  fetches: number;
  device: string | null;
  /// AirPlay only: the address fetching the file, i.e. the receiver chosen.
  peer?: string | null;
  /// AirPlay only: that receiver's `model`, from its own announcement.
  model?: string | null;
}

export type DeviceKind = 'tv' | 'box' | 'speaker';

export type CastVerdict =
  | { kind: 'direct'; transcodeAudio: boolean }
  | { kind: 'prepare'; transcodeAudio: boolean }
  | { kind: 'refuse'; reason: string };

class Cast {
  devices = $state<CastDevice[]>([]);
  dlnaDevices = $state<DlnaDevice[]>([]);
  discovering = $state(false);
  /// What the transport chosen for a device would mean for the file that is
  /// open — computed once when the picker opens, so each row can state its
  /// consequence without every row running an async probe.
  plan = $state<{
    /// **The resolved source, not `player.filePath`.** For a completed torrent
    /// those differ: the player holds a loopback URL and the cast would be
    /// pointed at the file on disk. Reasoning about the URL made
    /// `isNetworkSource` true and auto answered "Chromecast" for a file that
    /// actually goes over DLNA — the row contradicting what the click does.
    src: string;
    container: string;
    verdict: CastVerdict;
    /// The same question asked of AirPlay's decoder set (`airplayVerdict`);
    /// null off macOS.
    airplayVerdict: CastVerdict | null;
    streaming: boolean;
  } | null>(null);
  /// Which transport the live session is using; the controls, the poll and the
  /// disconnect all route on it.
  transport = $state<Transport>('cast');
  /// Bumped when a device profile changes, so rows recompute their line.
  profileRevision = $state(0);
  /// How many times discovery has been rebuilt during this search. The picker
  /// uses it to stop claiming that nothing is there: after a rebuild or two the
  /// honest statement is "still looking", not "not found".
  rebuilds = $state(0);
  /// The second discovery has not finished its first sweep, so a row that shows
  /// only Cast today may still gain DLNA. SSDP is a UDP broadcast answered at
  /// leisure, where mDNS answers at once — without saying so, the gear (and the
  /// consequence line under the name) appears seconds later for no visible
  /// reason, which reads as the panel changing its mind.
  dlnaSweeping = $state(false);
  /// The system sees at least one AirPlay receiver — the AirPlay row is shown
  /// only then. Which ones is not something macOS will say.
  airplayRoutes = $state(false);
  /// The AirPlay session plays an HLS rendition carrying every track, so the
  /// track menus switch the receiver itself — subtitles included.
  airplayTracks = $state(false);
  /// Bumped when the AirPlay settings or the receivers' memory change.
  airplayRevision = $state(0);
  /// A standing line on the casting screen for what the session gave up —
  /// a popup lasts a second while the viewer is looking at the television.
  notice = $state<string | null>(null);

  /// One row per television: the two discoveries merged by address.
  get tvs(): TvDevice[] {
    const byIp = new Map<string, TvDevice>();
    for (const c of this.devices) {
      byIp.set(c.ip, {
        key: deviceKey(c.id, c.name, c.model),
        name: c.name,
        model: c.model,
        ip: c.ip,
        cast: c,
      });
    }
    for (const d of this.dlnaDevices) {
      const existing = byIp.get(d.ip);
      if (existing) {
        existing.dlna = d;
        continue;
      }
      byIp.set(d.ip, {
        key: deviceKey(d.id, d.name, d.model),
        name: d.name,
        model: d.model,
        ip: d.ip,
        dlna: d,
      });
    }
    return [...byIp.values()];
  }

  /// A session exists, from connect until disconnect — the window is a remote.
  active = $state(false);
  deviceName = $state<string | null>(null);
  /// The session's device as its row drew it, for the same icon over the
  /// disconnect button. AirPlay's mark until the chosen set is known as a row.
  kind = $state<DeviceKind | 'airplay' | null>(null);
  /// Mirror of cast/castv2.rs's state string: connecting | connected | loading |
  /// buffering | playing | paused | ended | stopped | error.
  state = $state('idle');
  time = $state(0);
  duration = $state(0);
  volume = $state(1);
  muted = $state(false);
  /// Whether the receiver has reported a volume at all, and whether it
  /// declared it un-adjustable (`controlType: "fixed"`). Either negative
  /// disables the slider — a control that silently does nothing is worse
  /// than one that says why.
  volumeKnown = $state(false);
  volumeFixed = $state(false);

  get volumeAdjustable(): boolean {
    return this.volumeKnown && !this.volumeFixed;
  }

  get paused(): boolean {
    return this.state === 'paused';
  }
  get busy(): boolean {
    return (
      this.state === 'connecting' ||
      this.state === 'preparing' ||
      this.state === 'loading' ||
      this.state === 'buffering'
    );
  }
  /// The TV owns playback and this window is a remote. False while connecting
  /// or preparing, when local playback deliberately keeps running — the
  /// controls, wheel, keys and seekbar stay local until the handoff moment.
  get remote(): boolean {
    return this.active && this.state !== 'connecting' && this.state !== 'preparing';
  }
  /// The cast button's lit state. An AirPlay session exists from the click,
  /// while the viewer is still choosing in the system's list — a lit button
  /// then claims a television that may never be picked, so it lights only
  /// once one has taken over.
  get engaged(): boolean {
    return this.active && !(this.transport === 'airplay' && this.state === 'connecting');
  }
}

export const cast = new Cast();

function deviceKey(id: string, name: string, model: string): string {
  return id || `${name}|${model}`;
}

/**
 * What a session holds beside the reactive mirror. Plain, not `$state`: nothing
 * draws it. One object rather than module variables because the start, the
 * poll, the handback and the track switches live in different modules and
 * every one of them writes some of it.
 */
class Session {
  pollTimer: ReturnType<typeof setInterval> | null = null;
  loadStartedAt = 0;
  /// An AirPlay session became an external route. Until then the viewer is
  /// still choosing in the system's list and the local player keeps playing —
  /// so the fetch timeouts, which count from the handover, have not started.
  airplayLive = false;
  /// The HLS rendition's tracks by mpv id, in its master's order; null while the
  /// session plays the MP4 copy or the original file.
  airplayHls: { audioIds: number[]; subIds: number[] } | null = null;
  /// The receiver chosen, by the name it announces — what refusals are filed under.
  airplayReceiver: string | null = null;
  /// The kind of picture this session sends, for the refusal memory.
  airplayKind: PictureKind = 'sd';
  /// One fallback per file: a copy that is refused too is the session's end.
  airplayFellBack = false;
  /// Whether local playback was running when the cast started — what the
  /// handback resumes to.
  resumeUnpaused = false;
  /// The device this session belongs to. **The session is to the device, not to
  /// one file** — the queue moves under it, so advancing an episode is a re-LOAD
  /// rather than a disconnect and a fresh connect.
  currentDevice: TvDevice | null = null;
  /// The source the running cast was started from — what a mid-session audio
  /// switch re-prepares. The *served* path may be a prepared copy; this is the
  /// original (for a torrent: its resolved file on disk).
  castSrcPath: string | null = null;
  /// The session's transport: null = progressive MP4, else the HLS segment
  /// format ('ts' | 'fmp4'). Fixed at cast start from the mode setting; the
  /// audio switch follows it.
  sessionHls: 'ts' | 'fmp4' | null = null;
  /// The prepared copy served instead of the original this session, and whether
  /// its source is under a private root — a prepared copy of a private file must
  /// not outlive the session (casting.md, privacy).
  preparedPath: string | null = null;
  preparedHidden = false;
  /// The external subtitle a DLNA renderer was handed with the file, if any —
  /// what a pick in the track menu is compared with before the file is sent again.
  dlnaSubPath: string | null = null;
  /// Guard against the poll starting a second advance while one is in flight —
  /// the TV keeps reporting the old session as ended for the second or two the
  /// handover takes.
  advancing = false;
}

export const session = new Session();

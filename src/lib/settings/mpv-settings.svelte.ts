/// The mpv-backed half of the settings dialog: the rows that are lines of
/// mpv.conf (`SETTINGS`), their current values, and what the Video, Audio and
/// Subtitles tabs need to draw and change them — the audio device list, the
/// display's HDR state and the content-language picker.
///
/// A class rather than a module-level store, and instantiated by the dialog
/// rather than here: all of this means nothing while the sheet is shut, and it
/// used to be the dialog's own component state, so it has to start empty on
/// every opening exactly as that did. A module singleton would carry the last
/// opening's values into the next — and the HDR readout would then announce a
/// display change the viewer made while the sheet was closed.
import { invoke } from '@tauri-apps/api/core';
import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { refreshAudioOutput } from '$lib/audio-output.svelte';
import { IS_MAC } from '$lib/platform';
import { t } from '$lib/i18n.svelte';
import { mpvLangValue, parseLangList } from '$lib/languages';
import { player, readList } from '$lib/player.svelte';
import { showOsd } from '$lib/osd.svelte';

// Interactive settings editor: descriptions of the options mirrored into
// mpv.conf. v = null means "player default" (the line is commented out).
/// One row of the settings dialog. Two shapes, and the difference is what the
/// value IS: a pill picks one of several named values, a slider covers a
/// continuous range. Mixing them (a pill for "0.8 / 1.0 / 1.2") was the wrong
/// answer for subtitle size — the useful setting is "a bit bigger than that",
/// which a list of three cannot say.
interface SettingBase {
  /// Which section it appears in. Only the three that hold mpv settings.
  tab: 'video' | 'audio' | 'subs';
  /// The section inside the tab — see the `set-group` headings.
  group: 'picture' | 'decode' | 'online' | 'audio' | 'choice' | 'look';
  key: string;
  label: string;
  hint?: string;
  /// Value applied live when the setting is reset to the default.
  liveDefault: string;
}
interface SegmentedDef extends SettingBase {
  kind: 'segmented';
  options: { v: string | null; label: string }[];
}
/// Same control as a pill, laid out vertically because the labels are device
/// names ("MacBook Pro Speakers") rather than words, and because the list is
/// discovered at runtime and can be any length.
interface DeviceDef extends SettingBase {
  kind: 'devices';
}
/// An **ordered** list of content languages (`alang`, `slang`).
///
/// Not a pill row, and that is the whole reason this kind exists: `.segmented`
/// picks one of a few *named* values, and there are sixty-odd languages. It is
/// also not one value — mpv takes a priority list, and "Japanese, else
/// English" is the ordinary case for anyone watching anime, not an exotic one.
interface LangsDef extends SettingBase {
  kind: 'langs';
}
export interface SliderDef extends SettingBase {
  kind: 'slider';
  min: number;
  max: number;
  step: number;
  /// Digits in the readout. Also what makes the default reachable again: the
  /// step has to divide the distance from `min` to `liveDefault` exactly, or
  /// dragging back to it is impossible and the mpv.conf line never clears.
  decimals: number;
  suffix?: string;
}
export type SettingDef = SegmentedDef | DeviceDef | LangsDef | SliderDef;

const CONF_WRITE_MS = 400;

export class MpvSettings {
  // $derived, so switching the language re-renders the labels; the option
  // values that are not words (spline, AC3/EAC3) stay literal.
  SETTINGS: SettingDef[] = $derived([
    {
      kind: 'segmented',
      tab: 'video',
      key: 'target-colorspace-hint',

      group: 'picture',
      label: t('vset.hdr'),
      liveDefault: 'no',
      options: [
        { v: null, label: t('vset.hdr_sdr') },
        { v: 'auto', label: t('vset.hdr_auto') },
      ],
      hint: t(IS_MAC ? 'vset.hdr_hint_mac' : 'vset.hdr_hint_win'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'tone-mapping',

      group: 'picture',
      label: t('vset.tonemap'),
      liveDefault: 'spline',
      options: [
        { v: null, label: 'spline' },
        { v: 'bt.2446a', label: 'bt.2446a' },
        { v: 'bt.2390', label: 'bt.2390' },
      ],
      hint: t('vset.tonemap_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'ytdl-format',

      group: 'online',
      label: t('vset.quality'),
      // Empty is the hook's own choice, which on YouTube means the best
      // available — 4K wherever there is 4K, and every seek then pays for it.
      liveDefault: '',
      options: [
        { v: null, label: t('vset.quality_best') },
        { v: 'bestvideo[height<=?1080]+bestaudio/best', label: '1080p' },
        { v: 'bestvideo[height<=?720]+bestaudio/best', label: '720p' },
      ],
      hint: t('vset.quality_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'hwdec',

      group: 'decode',
      label: t('vset.hwdec'),
      liveDefault: 'auto-safe',
      // "Hardware" is hwdec=auto-safe, i.e. "take the hardware, but only where
      // mpv is confident about the decoder+output pairing". On macOS in our
      // build (output into an external NSView through --wid) auto-safe gives up
      // on VideoToolbox for some codecs and silently falls back to software —
      // on 4K60 that is seconds per exact seek. "VideoToolbox only" bypasses
      // that caution: mpv still falls back to software if the pairing fails.
      // What was actually chosen is shown at the bottom of this dialog
      // (`hwdec-current`).
      options: IS_MAC
        ? [
            { v: null, label: t('vset.hwdec_auto') },
            { v: 'videotoolbox', label: t('vset.hwdec_vt') },
            { v: 'no', label: t('vset.hwdec_sw') },
          ]
        : [
            { v: null, label: t('vset.hwdec_auto') },
            { v: 'no', label: t('vset.hwdec_sw') },
          ],
    },
    {
      kind: 'segmented',
      tab: 'audio',
      key: 'audio-spdif',

      group: 'audio',
      label: t('vset.spdif'),
      liveDefault: '',
      options: [
        { v: null, label: t('vset.spdif_off') },
        { v: 'ac3,eac3', label: 'AC3/EAC3' },
        { v: 'ac3,eac3,truehd', label: '+ TrueHD' },
      ],
      hint: t('vset.spdif_hint'),
    },
    {
      kind: 'devices',
      tab: 'audio',
      key: 'audio-device',

      group: 'audio',
      label: t('vset.device'),
      liveDefault: 'auto',
      hint: t('vset.device_hint'),
    },
    {
      kind: 'langs',
      tab: 'audio',
      key: 'alang',

      group: 'audio',
      label: t('vset.alang'),
      liveDefault: '',
      hint: t('vset.lang_hint'),
    },
    {
      kind: 'langs',
      tab: 'subs',
      key: 'slang',

      group: 'choice',
      label: t('vset.slang'),
      liveDefault: '',
      hint: t('vset.lang_hint'),
    },
    {
      kind: 'segmented',
      // Sits directly under `slang` because it modifies it, and it is mpv's
      // own option rather than selection logic of ours — reimplementing track
      // selection would fight `select_default_track` and the remembered-track
      // restore at once. Read from mpv 0.41's source rather than assumed: the
      // choices are `no`/`forced`/`yes` and the default is **yes**, so `null`
      // is the "показывать" pill and clearing the line restores it.
      tab: 'subs',
      key: 'subs-with-matching-audio',

      group: 'choice',
      label: t('vset.subs_matching'),
      liveDefault: 'yes',
      options: [
        { v: null, label: t('vset.subs_matching_yes') },
        { v: 'forced', label: t('vset.subs_matching_forced') },
        { v: 'no', label: t('vset.subs_matching_no') },
      ],
      hint: t('vset.subs_matching_hint'),
    },
    {
      kind: 'segmented',
      // `sid=no` rather than an `slang` value: turning subtitles off has to
      // survive external files too, and sub-auto selects those regardless of
      // language preferences.
      tab: 'subs',
      key: 'sid',

      group: 'choice',
      label: t('vset.subs_default'),
      liveDefault: 'auto',
      options: [
        { v: null, label: t('vset.subs_on') },
        { v: 'no', label: t('vset.subs_off') },
      ],
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-scale',

      group: 'look',
      label: t('vset.sub_scale'),
      // mpv's own default, and its `sub-ass-override` default is `scale`, so
      // this reaches ASS subtitles too rather than only plain text ones.
      liveDefault: '1',
      min: 0.5,
      max: 2,
      step: 0.05,
      decimals: 2,
      suffix: '×',
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-pos',

      group: 'look',
      label: t('vset.sub_pos'),
      liveDefault: '100',
      min: 0,
      max: 150,
      step: 1,
      decimals: 0,
      hint: t('vset.sub_pos_hint'),
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-border-size',

      group: 'look',
      label: t('vset.sub_border'),
      // 1.65 is mpv's default here — not the 3 it used to be, and the step has
      // to land on it exactly for "back to default" to be reachable.
      liveDefault: '1.65',
      min: 0,
      max: 5,
      step: 0.05,
      decimals: 2,
    },
    {
      kind: 'segmented',
      tab: 'subs',
      key: 'sub-border-style',

      group: 'look',
      label: t('vset.sub_style'),
      liveDefault: 'outline-and-shadow',
      options: [
        { v: null, label: t('vset.sub_style_outline') },
        { v: 'background-box', label: t('vset.sub_style_box') },
      ],
      hint: t('vset.sub_style_hint'),
    },
  ]);

  /// Menu items do nothing on their own — Rust sends the id here and the same
  /// code as the context menu performs the action.

  settingsValues = $state<Record<string, string | null>>({});

  // HDR state of the display under the window: while it is in SDR, HDR output
  // is impossible (target-colorspace-hint is a no-op, mpv always tone-maps) —
  // which the settings dialog spells out.
  displayHdr = $state<{ supported: boolean; enabled: boolean } | null>(null);
  /// Output devices, read when the dialog opens. mpv builds this list lazily —
  /// the hotplug monitor starts on the first read — so there is nothing to
  /// observe and no reason to keep it fresh while the dialog is closed.
  audioDevices = $state<{ name: string; description: string }[]>([]);

  /**
   * Output devices, deduplicated by what they are called.
   *
   * mpv lists a device once per audio output that can reach it, and the macOS
   * build carries three — `coreaudio`, `coreaudio_exclusive` and
   * `avfoundation` (verified in the bundled libmpv). The `name` differs by that
   * prefix while the `description` is identical, so the picker showed every
   * speaker two or three times with nothing to tell the copies apart.
   *
   * The first entry for a description wins, which is mpv's own preferred
   * output. What that gives up is exclusive mode, and deliberately: it is an
   * audiophile setting that takes the device away from every other app, it
   * cannot be labeled in this dialog without inventing jargon, and anyone who
   * wants it can name the device in mpv.conf — which is what the footer of this
   * dialog already says is where mpv's own settings live.
   */
  async loadAudioDevices() {
    const all = await readList('audio-device-list', async (base) => {
      const name = await getProperty(`${base}/name`, 'string').catch(() => null);
      if (!name) return null;
      const description = await getProperty(`${base}/description`, 'string').catch(() => null);
      return { name, description: description?.trim() || name };
    }).catch(() => []);
    const seen = new Set<string>();
    this.audioDevices = all.filter((device) => {
      if (seen.has(device.description)) return false;
      seen.add(device.description);
      return true;
    });
  }

  async refreshHdrStatus() {
    const st = await invoke<{ supported: boolean; enabled: boolean }>('hdr_status').catch(
      () => null,
    );
    if (!st) return;
    const prev = this.displayHdr;
    this.displayHdr = st;
    if (prev && prev.enabled !== st.enabled) {
      showOsd(t(st.enabled ? 'osd.display_hdr' : 'osd.display_sdr'));
      // Embedded through wid, mpv may not notice the HDR switch
      // (WM_DISPLAYCHANGE is only broadcast to top-level windows) — re-apply
      // the hint so the VO re-reads the output color space.
      const hint =
        (await getProperty('target-colorspace-hint', 'string').catch(() => null)) ?? 'no';
      if (player.hasFile && hint !== 'no') {
        try {
          await command('set', ['target-colorspace-hint', 'no']);
          await command('set', ['target-colorspace-hint', hint]);
        } catch (e) {
          console.warn('hdr re-hint failed:', e);
        }
      }
    }
  }

  /// Pending mpv.conf writes, one timer per key. A slider fires on every pixel
  /// of the drag: applying that live is free, but writing the file each time is
  /// not — and a single shared timer would let a second slider cancel the first
  /// one's write.
  #confWrites = new Map<string, ReturnType<typeof setTimeout>>();

  setSetting(def: SettingDef, v: string | null, debounce = false) {
    this.settingsValues = { ...this.settingsValues, [def.key]: v };
    // apply live, without a restart
    const applied = command('set', [def.key, v ?? def.liveDefault]).catch(() => {});
    // Another device has its own offset (audio-output.svelte.ts).
    if (def.key === 'audio-device') void applied.then(() => refreshAudioOutput());
    // persist into mpv.conf (surgically, leaving the rest alone)
    clearTimeout(this.#confWrites.get(def.key));
    const write = () => {
      this.#confWrites.delete(def.key);
      void invoke('mpv_conf_set', { key: def.key, value: v }).catch((e) => {
        showOsd(t('osd.setting_failed'));
        console.warn('mpv_conf_set failed:', e);
      });
    };
    if (debounce) this.#confWrites.set(def.key, setTimeout(write, CONF_WRITE_MS));
    else write();
  }

  // ---- Content languages (ROADMAP 25) --------------------------------------
  //
  // `alang`/`slang` are ordered priority lists, and what is stored is a plain
  // mpv.conf line — so the list is *derived* from the line rather than kept
  // beside it. That is what makes a value someone typed into their own mpv.conf
  // show up here correctly, and it is why nothing needs migrating: the two
  // values this dialog used to write, `rus,ru` and `eng,en`, parse back to
  // exactly [ru] and [en].

  /// Which setting's language list is open for adding, and what has been typed.
  /// One at a time: two open search panels in a dialog this size is noise.
  langPickerFor = $state<string | null>(null);
  langQuery = $state('');

  langsOf(def: SettingDef): string[] {
    return parseLangList(this.settingsValues[def.key] ?? null);
  }

  /// Write a language list back. An empty list clears the mpv.conf line rather
  /// than writing an empty one — "авто" is the absence of the setting, the same
  /// meaning "по умолчанию" has in every pill in this dialog.
  setLangs(def: SettingDef, codes: string[]) {
    this.setSetting(def, codes.length ? mpvLangValue(codes) : null);
  }

  addLang(def: SettingDef, code: string) {
    this.setLangs(def, [...this.langsOf(def), code]);
    this.langPickerFor = null;
    this.langQuery = '';
  }

  removeLang(def: SettingDef, code: string) {
    this.setLangs(
      def,
      this.langsOf(def).filter((c) => c !== code),
    );
  }

  /// Clicking a chip makes it the preferred language.
  ///
  /// Order is priority, and without this the only way to change it is to remove
  /// every language above the one you want and add them back — which also
  /// appends them, so it does not even work. One click on the thing you want
  /// beats a pair of arrows on every chip, in a row that is usually two items
  /// long.
  promoteLang(def: SettingDef, code: string) {
    const codes = this.langsOf(def);
    if (codes[0] === code) return;
    this.setLangs(def, [code, ...codes.filter((c) => c !== code)]);
  }

  /// Current value of a slider: what mpv.conf says, or mpv's own default.
  sliderValue(def: SliderDef): number {
    const saved = this.settingsValues[def.key];
    const value = Number(saved ?? def.liveDefault);
    return Number.isFinite(value) ? Math.min(def.max, Math.max(def.min, value)) : Number(def.liveDefault);
  }

  setSlider(def: SliderDef, value: number) {
    // Landing exactly on mpv's own default removes the line from mpv.conf
    // rather than pinning the default in writing — the same meaning "по
    // умолчанию" has in every pill above.
    // Snapped to the declared precision before it is written: a range input
    // can hand back 1.2000000000000002, and that would go into mpv.conf as is.
    const snapped = Number(value.toFixed(def.decimals));
    const isDefault = Math.abs(snapped - Number(def.liveDefault)) < 1e-9;
    this.setSetting(def, isDefault ? null : String(snapped), true);
  }
}

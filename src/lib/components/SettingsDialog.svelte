<script lang="ts">
  /// The settings dialog: everything the player lets you change, plus the
  /// hotkey editor.
  ///
  /// All of its state lives here — the mirrored mpv.conf values, the HDR
  /// readout, the audio device list, the language picker and the key recorder —
  /// because none of it means anything while the dialog is shut. Mounting is
  /// opening: the caller renders this only while the sheet is up, so reading
  /// the current values, polling the display and tearing the key recorder down
  /// are all just this component's lifecycle.
  ///
  /// One deliberate behavior change came with that. The display's HDR state
  /// used to be polled every three seconds for the whole life of the app; it is
  /// only ever shown here, so the timer now runs only while the dialog is open.
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { command, getProperty } from 'tauri-plugin-libmpv-api';

  import Dialog from '$lib/components/Dialog.svelte';
  import ScrollFade from '$lib/components/ScrollFade.svelte';
  import { IS_MAC } from '$lib/platform';
  import { locale, setLocale, t, type Locale } from '$lib/i18n.svelte';
  import {
    ACTIONS,
    GROUP_ORDER,
    actionLabel,
    assign,
    chordLabel,
    chordOf,
    chordsOf,
    hasCustomBindings,
    isCustom,
    isDigitJump,
    isMenuAccelerator,
    isModifierCode,
    reservedReason,
    resetAction,
    resetAll,
    unassign,
    type ActionId,
  } from '$lib/keys.svelte';
  import { languageName, mpvLangValue, parseLangList, searchLanguages } from '$lib/languages';
  import {
    addExcludedFolder,
    clearHistory,
    history,
    removeExcludedFolder,
    toggleHistory,
  } from '$lib/history.svelte';
  import { applyNormalize, player, readList } from '$lib/player.svelte';
  import { playlist, setPlaylistPref } from '$lib/playlist.svelte';
  import { refreshPortStatus, torrent, torrentPrefs } from '$lib/torrent.svelte';
  import { pickTorrentDir, resetTorrentDir } from '$lib/open.svelte';
  import { castCacheCapGb, setCastCacheCapGb } from '$lib/cast.svelte';
  import { showOsd } from '$lib/osd.svelte';
  import { syncMenuChecks } from '$lib/window-prefs.svelte';
  import { DEFAULT_RELAY, relayUrl, setRelayUrl } from '$lib/sync/wire.svelte';
  import {
    DEFAULT_INDEXER,
    DEFAULT_TMDB,
    catalog,
    indexerUrl,
    setCatalogEnabled,
    setIndexerUrl,
    setTmdbUrl,
    tmdbUrl,
  } from '$lib/catalog.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    onclose: () => void;
    /// Page-owned because they act on the player and the torrent session, not
    /// on a setting: one stops the session and rebuilds it, the other deletes
    /// data off the disk.
    onToggleSeeding: () => void;
    onTogglePortForward: () => void;
    onClearTorrentCache: () => void;
    /// Raises the third-party notices, which are a layer above this sheet. A
    /// callback rather than reaching for `overlays`: no component in this
    /// project opens a dialog by itself — the page owns the surface stack.
    onLicenses: () => void;
  }

  let { onclose, onToggleSeeding, onTogglePortForward, onClearTorrentCache, onLicenses }: Props =
    $props();

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
  interface SliderDef extends SettingBase {
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
  type SettingDef = SegmentedDef | DeviceDef | LangsDef | SliderDef;

  // $derived, so switching the language re-renders the labels; the option
  // values that are not words (spline, AC3/EAC3) stay literal.
  const SETTINGS: SettingDef[] = $derived([
    {
      kind: 'segmented',
      tab: 'video',
      key: 'target-colorspace-hint',
      label: t('vset.hdr'),
      liveDefault: 'no',
      options: [
        { v: null, label: t('vset.hdr_sdr') },
        { v: 'auto', label: t('vset.hdr_auto') },
      ],
      hint: t('vset.hdr_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'tone-mapping',
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
      label: t('vset.device'),
      liveDefault: 'auto',
      hint: t('vset.device_hint'),
    },
    {
      kind: 'langs',
      tab: 'audio',
      key: 'alang',
      label: t('vset.alang'),
      liveDefault: '',
      hint: t('vset.lang_hint'),
    },
    {
      kind: 'langs',
      tab: 'subs',
      key: 'slang',
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

  let settingsValues = $state<Record<string, string | null>>({});

  // HDR state of the display under the window: while it is in SDR, HDR output
  // is impossible (target-colorspace-hint is a no-op, mpv always tone-maps) —
  // which the settings dialog spells out.
  let displayHdr = $state<{ supported: boolean; enabled: boolean } | null>(null);
  /// Output devices, read when the dialog opens. mpv builds this list lazily —
  /// the hotplug monitor starts on the first read — so there is nothing to
  /// observe and no reason to keep it fresh while the dialog is closed.
  let audioDevices = $state<{ name: string; description: string }[]>([]);

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
  async function loadAudioDevices() {
    const all = await readList('audio-device-list', async (base) => {
      const name = await getProperty(`${base}/name`, 'string').catch(() => null);
      if (!name) return null;
      const description = await getProperty(`${base}/description`, 'string').catch(() => null);
      return { name, description: description?.trim() || name };
    }).catch(() => []);
    const seen = new Set<string>();
    audioDevices = all.filter((device) => {
      if (seen.has(device.description)) return false;
      seen.add(device.description);
      return true;
    });
  }


  /// The decoder mpv actually picked (`hwdec-current`), shown in the settings:
  /// exact-seek speed depends on it — on 4K60 the difference between hardware
  /// and software decoding is plainly visible, and a silent fallback to
  /// software was previously impossible to notice.
  let hwdecCurrent = $state<string | null>(null);
  /// Settings dialog tab, split by who owns the setting: "Video" is what ends
  /// up in mpv.conf, "General" is what the player keeps itself. "General" by
  /// default: video options are fine-tuning, reached deliberately, whereas the
  /// first tab is more often opened by accident.
  /// Which section of the settings dialog is showing. Topical rather than the
  /// old two-way split by who owns the value — the ownership boundary is still
  /// real (only some of these end up in mpv.conf) but it is a poor map for
  /// someone looking for "subtitle size", which is what a settings dialog is
  /// searched by. What the boundary still owns is the footer: it now names mpv's
  /// settings rather than claiming everything on the page is one of them.
  type SettingsTab = 'general' | 'video' | 'playback' | 'audio' | 'subs' | 'torrents' | 'tv' | 'keys';
  let settingsTab = $state<SettingsTab>('general');
  // The tab row is nowrap with hidden overflow, and the sheet's width is sized
  // from it (598px fits seven Russian labels with ~50px slack) — which is why
  // this tab is «ТВ», not «Трансляция»: two characters ride in the slack, a
  // ten-character label overflows and the eighth tab is silently gone.
  const SETTINGS_TABS: { id: SettingsTab; label: string }[] = $derived([
    { id: 'general', label: t('set.tab_general') },
    { id: 'video', label: t('set.tab_video') },
    { id: 'playback', label: t('set.tab_playback') },
    { id: 'audio', label: t('set.tab_audio') },
    { id: 'subs', label: t('set.tab_subs') },
    { id: 'torrents', label: t('set.tab_torrents') },
    { id: 'tv', label: t('set.tab_tv') },
    { id: 'keys', label: t('set.tab_keys') },
  ]);

  // ---- Watching together ----
  // Written on `change` rather than on every keystroke: a half-typed host is a
  // host, and storing it means the room dialog would try to open it if the
  // viewer left the sheet without blurring the field.
  let relayVal = $state(relayUrl());

  function saveRelay(next: string) {
    setRelayUrl(next);
    // Read back rather than kept: `setRelayUrl` trims and drops a trailing
    // slash, and an empty entry falls back to the default — so the field has to
    // show what will actually be used, not what was typed.
    relayVal = relayUrl();
  }

  // ---- The catalog ----
  // Same shape as the relay above and for the same reason: a half-typed host is
  // a host, and the field has to show what will actually be used rather than
  // what was typed — an empty entry falls back to the default rather than
  // turning the feature off, which is what the switch beside it is for.
  let indexerVal = $state(indexerUrl());
  let tmdbVal = $state(tmdbUrl());

  function saveIndexer(next: string) {
    setIndexerUrl(next);
    indexerVal = indexerUrl();
  }

  function saveTmdb(next: string) {
    setTmdbUrl(next);
    tmdbVal = tmdbUrl();
  }

  // ---- The TV (casting) tab ----
  let castCapVal = $state(castCacheCapGb());
  let castCacheBytes = $state<number | null>(null);
  const CAST_CAP_CHOICES = [0, 5, 20, 50];


  function setCastCapHere(gb: number) {
    castCapVal = gb;
    setCastCacheCapGb(gb);
  }

  async function refreshCastCacheSize() {
    castCacheBytes = await invoke<number>('cast_cache_size').catch(() => null);
  }

  $effect(() => {
    if (settingsTab === 'tv') void refreshCastCacheSize();
  });

  async function clearCastCacheHere() {
    const freed = await invoke<number>('cast_clear_cache').catch(() => 0);
    showOsd(t('cast.cache_cleared', { size: fmtSize(freed) }));
    void refreshCastCacheSize();
  }
  // Popup elements: measured after render (before paint) to clamp to the window

  async function loadSettings() {
    void loadAudioDevices();
    try {
      const conf = await invoke<{ path: string; options: [string, string][] }>('user_mpv_conf');
      player.mpvConfPath = conf.path;
      const map: Record<string, string | null> = {};
      for (const s of SETTINGS) map[s.key] = null;
      for (const [k, v] of conf.options) if (k in map) map[k] = v;
      settingsValues = map;
      void refreshHdrStatus();
      hwdecCurrent = player.hasFile
        ? ((await getProperty('hwdec-current', 'string').catch(() => null)) ?? null)
        : null;
    } catch (e) {
      // The sheet is already on screen — mounting is what opened it — so a
      // failure here has to take it away again. Before the split this simply
      // declined to set `settingsOpen`, which had the same effect.
      showOsd(t('osd.settings_failed'));
      console.warn('reading the settings failed:', e);
      onclose();
    }
  }

  /// Read the current values once, and keep the display's HDR state fresh while
  /// the sheet is up: it reacts to the Windows HDR toggle and to the window
  /// moving to another monitor, and on macOS the EDR headroom genuinely
  /// fluctuates. The call is microseconds.
  $effect(() => {
    void loadSettings();
    const timer = setInterval(() => void refreshHdrStatus(), 3000);
    return () => clearInterval(timer);
  });

  async function refreshHdrStatus() {
    const st = await invoke<{ supported: boolean; enabled: boolean }>('hdr_status').catch(
      () => null,
    );
    if (!st) return;
    const prev = displayHdr;
    displayHdr = st;
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
  const confWrites = new Map<string, ReturnType<typeof setTimeout>>();
  const CONF_WRITE_MS = 400;

  function setSetting(def: SettingDef, v: string | null, debounce = false) {
    settingsValues = { ...settingsValues, [def.key]: v };
    // apply live, without a restart
    void command('set', [def.key, v ?? def.liveDefault]).catch(() => {});
    // persist into mpv.conf (surgically, leaving the rest alone)
    clearTimeout(confWrites.get(def.key));
    const write = () => {
      confWrites.delete(def.key);
      void invoke('mpv_conf_set', { key: def.key, value: v }).catch((e) => {
        showOsd(t('osd.setting_failed'));
        console.warn('mpv_conf_set failed:', e);
      });
    };
    if (debounce) confWrites.set(def.key, setTimeout(write, CONF_WRITE_MS));
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
  let langPickerFor = $state<string | null>(null);
  let langQuery = $state('');
  let langQueryEl = $state<HTMLInputElement | undefined>();

  function langsOf(def: SettingDef): string[] {
    return parseLangList(settingsValues[def.key] ?? null);
  }

  /// Write a language list back. An empty list clears the mpv.conf line rather
  /// than writing an empty one — "авто" is the absence of the setting, the same
  /// meaning "по умолчанию" has in every pill in this dialog.
  function setLangs(def: SettingDef, codes: string[]) {
    setSetting(def, codes.length ? mpvLangValue(codes) : null);
  }

  async function openLangPicker(def: SettingDef) {
    langPickerFor = langPickerFor === def.key ? null : def.key;
    langQuery = '';
    if (!langPickerFor) return;
    await tick();
    langQueryEl?.focus();
  }

  function addLang(def: SettingDef, code: string) {
    setLangs(def, [...langsOf(def), code]);
    langPickerFor = null;
    langQuery = '';
  }

  function removeLang(def: SettingDef, code: string) {
    setLangs(
      def,
      langsOf(def).filter((c) => c !== code),
    );
  }

  /// Clicking a chip makes it the preferred language.
  ///
  /// Order is priority, and without this the only way to change it is to remove
  /// every language above the one you want and add them back — which also
  /// appends them, so it does not even work. One click on the thing you want
  /// beats a pair of arrows on every chip, in a row that is usually two items
  /// long.
  function promoteLang(def: SettingDef, code: string) {
    const codes = langsOf(def);
    if (codes[0] === code) return;
    setLangs(def, [code, ...codes.filter((c) => c !== code)]);
  }

  /// Current value of a slider: what mpv.conf says, or mpv's own default.
  function sliderValue(def: SliderDef): number {
    const saved = settingsValues[def.key];
    const value = Number(saved ?? def.liveDefault);
    return Number.isFinite(value) ? Math.min(def.max, Math.max(def.min, value)) : Number(def.liveDefault);
  }

  function setSlider(def: SliderDef, value: number) {
    // Landing exactly on mpv's own default removes the line from mpv.conf
    // rather than pinning the default in writing — the same meaning "по
    // умолчанию" has in every pill above.
    // Snapped to the declared precision before it is written: a range input
    // can hand back 1.2000000000000002, and that would go into mpv.conf as is.
    const snapped = Number(value.toFixed(def.decimals));
    const isDefault = Math.abs(snapped - Number(def.liveDefault)) < 1e-9;
    setSetting(def, isDefault ? null : String(snapped), true);
  }

  /// The language also lives in the native macOS menu, which is built in Rust
  /// and cannot reach the dictionary — push the choice over and let it rebuild.
  function changeLocale(next: Locale) {
    setLocale(next);
    if (IS_MAC) {
      void invoke('set_menu_locale', { locale: next })
        .then(() => syncMenuChecks())
        .catch(() => {});
    }
  }


  // ---- Key bindings editor -------------------------------------------------

  /// The action whose next keystroke is being captured, or null.
  let recordingAction = $state<ActionId | null>(null);
  /// One line under one row: what a rebind took away, or why a chord was
  /// refused. Cleared by the next attempt, so it reads as a reply rather than
  /// as a status the panel keeps.
  let keyNote = $state<{ id: ActionId; text: string } | null>(null);

  const RESERVED_NOTE = {
    digits: 'keys.taken_digits',
    contextual: 'keys.taken_contextual',
    menu: 'keys.taken_menu',
  } as const;

  /// Capture phase, on the window: the player's own handler is a *bubble*
  /// listener on the same window, and stopping propagation on the way down
  /// means the keystroke never reaches it. Without that, recording a binding
  /// for "полный экран" would go fullscreen while recording it. `preventDefault`
  /// is the other half — Space would otherwise re-click the button that started
  /// the recording, and Tab would move focus out of the row.
  function recordChord(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    // A modifier on its own is not a chord: the viewer is still reaching for
    // the key, so keep waiting rather than binding ⇧ to anything.
    if (isModifierCode(e.code)) return;
    const id = recordingAction;
    if (!id) return;
    const chord = chordOf(e);
    // Bare Escape cancels. It is reserved anyway, so without this the panel
    // would answer an attempt to back out by explaining why Escape is taken.
    if (chord === 'Escape') {
      stopRecording();
      return;
    }
    const reason = reservedReason(chord);
    if (reason) {
      keyNote = { id, text: t(RESERVED_NOTE[reason]) };
      stopRecording();
      return;
    }
    const stolen = assign(id, chord);
    keyNote = stolen ? { id, text: t('keys.stolen', { action: actionLabel(stolen) }) } : null;
    stopRecording();
  }

  function startRecording(id: ActionId) {
    keyNote = null;
    recordingAction = id;
    window.addEventListener('keydown', recordChord, true);
  }

  function stopRecording() {
    recordingAction = null;
    window.removeEventListener('keydown', recordChord, true);
  }

  $effect(() => {
    // Recording must not outlive the panel that started it: a capture listener
    // left on the window would swallow the next key pressed at the video, which
    // reads as the player having frozen rather than as a dialog left open.
    void settingsTab;
    return stopRecording;
  });

  /// Same rule for the language search, and for the same reason: a panel left
  /// open belongs to the tab that opened it. Reopening the dialog onto a
  /// half-typed query would also be a state nobody asked for.
  $effect(() => {
    void settingsTab;
    return () => {
      langPickerFor = null;
      langQuery = '';
    };
  });

  /// Ask the router when the section that shows the answer comes up, and only
  /// then: it is an SSDP search plus a SOAP round trip, and asking on every
  /// settings opening would spend it on six tabs out of seven. Re-reads on each
  /// visit rather than caching, because the answer legitimately changes — a
  /// mapping appears once the first torrent builds the session.
  $effect(() => {
    if (settingsTab === 'torrents') {
      void refreshPortStatus();
      void readTorrentDir();
    }
  });

  /// Where torrents are written, read from Rust rather than kept here: the
  /// preference lives beside the data (a dot-file in the state directory) so
  /// that a command arriving before this dialog has ever been opened still puts
  /// files in the right place.
  let torrentDir = $state<string | null>(null);
  let torrentDirDefault = $state(true);
  async function readTorrentDir() {
    const answer = await invoke<[string, boolean]>('torrent_dir').catch(() => null);
    if (!answer) return;
    // Assigned one at a time rather than destructured: `check-runes` reads
    // writes syntactically, and a destructuring assignment is not one it sees —
    // which would report this state as never written and make the gate a
    // warning people learn to ignore.
    torrentDir = answer[0];
    torrentDirDefault = answer[1];
  }

  /// The sentence under the switch. `null` while it is off — a row that is off
  /// has nothing to report, and a line saying so would be noise on the setting
  /// most people will never turn on.
  const portLine = $derived.by(() => {
    if (!torrentPrefs.portForward) return null;
    if (torrent.portChecking) return t('torrent.port_checking');
    const s = torrent.portStatus;
    if (!s) return null;
    if (s.state === 'mapped') {
      return t('torrent.port_mapped', { port: s.port, detail: s.detail ?? '' });
    }
    if (s.state === 'unmapped') return t('torrent.port_unmapped', { port: s.port });
    if (s.state === 'no_router') return t('torrent.port_no_router');
    return t('torrent.port_no_session');
  });
</script>

<Dialog title={t('set.title')} scrollable {onclose}>
  <div class="tabs" role="tablist">
    {#each SETTINGS_TABS as tab (tab.id)}
      <button
        class="tab"
        class:sel={settingsTab === tab.id}
        role="tab"
        aria-selected={settingsTab === tab.id}
        onclick={() => (settingsTab = tab.id)}
      >
        {tab.label}
      </button>
    {/each}
  </div>
  {#if settingsTab === 'general'}
    <div class="setting">
      <div class="setting-label">{t('set.language')}</div>
      <div class="segmented">
        <button class="segopt" class:sel={locale() === 'ru'} onclick={() => changeLocale('ru')}>
          Русский
        </button>
        <button class="segopt" class:sel={locale() === 'en'} onclick={() => changeLocale('en')}>
          English
        </button>
      </div>
    </div>

    <!-- Watching together needs a server both ends agree on, and this is where
         it lives rather than in the room dialog: practically nobody runs their
         own, so a field on the way into every room was asking a question with
         one answer. The room dialog points here when the address turns out to
         be wrong, which is the only moment it matters.

         Placed with the language rather than inside the history block below,
         even though "which server learns what I watch" is a fair privacy
         question: history, excluded folders and clearing them are one story
         read top to bottom, and a text field in the middle of it breaks the
         run.

         Empty means the default — `setRelayUrl` removes the key rather than
         storing a blank — so the placeholder is the address itself and
         clearing the field restores it instead of turning the feature off. -->
    <div class="setting">
      <div class="setting-label">{t('sync.relay_label')}</div>
      <div class="setting-hint">{t('sync.relay_hint')}</div>
      <input
        class="link-input"
        value={relayVal}
        placeholder={DEFAULT_RELAY}
        spellcheck="false"
        autocapitalize="off"
        aria-label={t('sync.relay_label')}
        onchange={(e) => saveRelay(e.currentTarget.value)}
      />
    </div>


    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('set.history')}</div>
          <div class="setting-hint">{t('set.history_hint')}</div>
        </div>
        <button
          class="switch"
          class:on={history.prefs.enabled}
          role="switch"
          aria-checked={history.prefs.enabled}
          aria-label={t('set.history')}
          onclick={toggleHistory}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    <!-- Dimmed together with disabled history: when nothing is written,
         the exclusions affect nothing, and that should be visible. -->
    <div class="setting" class:muted={!history.prefs.enabled}>
      <div class="setting-label">{t('set.excluded')}</div>
      <div class="setting-hint">{t('set.excluded_hint')}</div>
      <div class="folders">
        {#each history.prefs.excluded as dir (dir)}
          <div class="folder-row">
            <svg class="folder-ico" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M1.75 3.5h3.9l1.2 1.6h7.4v7.4H1.75z"
                fill="none"
                stroke="currentColor"
                stroke-width="1.3"
                stroke-linejoin="round"
              />
            </svg>
            <!-- bdi isolates the text direction: the container is rtl (so
                 the ellipsis lands on the left, since the meaningful part
                 of a path is its tail), and without isolation the slashes,
                 being neutral characters, move about and "/Users/…" is
                 drawn as "Users/…/". -->
            <span class="folder-path" title={dir}><bdi>{dir}</bdi></span>
            <button
              class="folder-remove"
              data-tip={t('set.excluded_remove')}
              aria-label={t('set.excluded_remove')}
              disabled={!history.prefs.enabled}
              onclick={() => removeExcludedFolder(dir)}
            >
              <!-- Same coordinates as the card's cross: round-capped ends
                   must stay inside the viewBox, or they get clipped at
                   the edge. -->
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
              </svg>
            </button>
          </div>
        {:else}
          <div class="folders-empty">{t('set.excluded_empty')}</div>
        {/each}
        <button class="folder-add" disabled={!history.prefs.enabled} onclick={addExcludedFolder}>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M8 3.5v9M3.5 8h9" />
          </svg>
          {t('set.excluded_add')}
        </button>
      </div>
    </div>

    <div class="setting">
      <div class="setting-label">{t('set.clear')}</div>
      <div class="setting-hint">{t('set.clear_hint')}</div>
      <button class="btn-danger" onclick={() => void clearHistory()}>{t('set.clear_btn')}</button>
    </div>

  {:else if settingsTab === 'torrents'}
    <!-- First in the tab, because "how do I find a release" comes before
         "what happens to one I have". On by default, but the switch stays:
         this is the only surface in the player that tells a third party what
         somebody is *looking for* rather than acting on a file they already
         hold, which is an argument for a way out rather than for hiding the
         feature from everyone who never opens this sheet. -->
    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('catalog.setting')}</div>
          <div class="setting-hint">{t('catalog.setting_hint')}</div>
        </div>
        <button
          class="switch"
          class:on={catalog.enabled}
          role="switch"
          aria-checked={catalog.enabled}
          aria-label={t('catalog.setting')}
          onclick={() => setCatalogEnabled(!catalog.enabled)}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    <!-- Shown only once the catalog is on: an address for a feature nobody
         has enabled is a question about something that is not happening. -->
    {#if catalog.enabled}
      <!-- The metadata service first, because it is the one the player itself
           depends on: without it the panel still works but has no pictures,
           which is the difference a viewer notices immediately. -->
      <div class="setting">
        <div class="setting-label">{t('catalog.tmdb_label')}</div>
        <div class="setting-hint">{t('catalog.tmdb_hint')}</div>
        <input
          class="link-input"
          value={tmdbVal}
          placeholder={DEFAULT_TMDB}
          spellcheck="false"
          autocapitalize="off"
          aria-label={t('catalog.tmdb_label')}
          onchange={(e) => saveTmdb(e.currentTarget.value)}
        />
      </div>

      <div class="setting">
        <div class="setting-label">{t('catalog.indexer_label')}</div>
        <div class="setting-hint">{t('catalog.indexer_hint')}</div>
        <!-- The placeholder is whatever is *actually* being used when the
             viewer has set nothing — the service's suggestion, which is not a
             constant and may be withdrawn. Showing it here is the only place
             the effective value is visible, and an empty box with no
             placeholder would read as "nothing configured" while the catalog
             plainly works. -->
        <input
          class="link-input"
          value={indexerVal}
          placeholder={catalog.suggested || DEFAULT_INDEXER}
          spellcheck="false"
          autocapitalize="off"
          aria-label={t('catalog.indexer_label')}
          onchange={(e) => saveIndexer(e.currentTarget.value)}
        />
      </div>
    {/if}

    <!-- Seeding was under the privacy controls, on the argument that it is
         the same kind of decision as an excluded folder — what leaves this
         machine. That argument still holds and is not why it moved: a
         viewer looking for anything about torrents had no section to look
         in, and found the switch and the cache button by reading a tab
         named "Основные" to the bottom. The two are here together because
         they are the whole of what this player decides about a torrent —
         what goes out, and what stays on the disk. A switch, because it is
         on/off; the hint carries the legal weight. -->
    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('torrent.seed')}</div>
          <div class="setting-hint">{t('torrent.seed_hint')}</div>
        </div>
        <button
          class="switch"
          class:on={torrentPrefs.seeding}
          role="switch"
          aria-checked={torrentPrefs.seeding}
          aria-label={t('torrent.seed')}
          onclick={onToggleSeeding}
        >
          <!-- `.switch-knob`, like every other switch in this dialog. A
               bare `.knob` has no rule anywhere, so this one rendered as a
               track with nothing in it — on/off told apart only by the
               background. -->
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    <!-- The only large lever left on peer count, and it is measured rather
         than believed: of ~30 addresses a rutracker announce returned, 20–22
         never answered a SYN — peers behind NAT, which can only ever dial us.
         Public trackers, `numwant`, IPv6 and PEX were all measured and give
         nothing here.

         It carries a status line because the switch alone would be a claim:
         librqbit's forwarder reports to nobody, and a router with UPnP
         disabled swallows the request in silence. "On" and "working" are
         different facts, so the row says both. -->
    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('torrent.port')}</div>
          <div class="setting-hint">{t('torrent.port_hint')}</div>
          {#if portLine}
            <div class="setting-hint port-state" class:ok={torrent.portStatus?.state === 'mapped'}>
              {portLine}
            </div>
          {/if}
        </div>
        <button
          class="switch"
          class:on={torrentPrefs.portForward}
          role="switch"
          aria-checked={torrentPrefs.portForward}
          aria-label={t('torrent.port')}
          onclick={onTogglePortForward}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    <!-- **A setting rather than a question asked at the first torrent.** The
         player has to work with nothing configured, and stopping somebody who
         wants to watch an episode to ask about storage is a modal at the worst
         moment — qBittorrent asks because it *is* a download manager. What the
         default costs is discoverability: the cache directory is one the system
         may empty and nobody browses by hand, so the path is printed here in
         full and the folder button on the start screen opens it. -->
    <div class="setting">
      <div class="setting-label">{t('torrent.dir')}</div>
      <div class="setting-hint">{t('torrent.dir_hint')}</div>
      <!-- **Shaped as a field with its action inside it**, rather than as a
           row of text with a button underneath. What it shows is a value that
           can be changed, which is what a field looks like — and the excluded
           folders' row, borrowed here first, is built for a *list*: short, and
           inset by an icon it needs to distinguish one entry from the next.
           There is one entry here and a label above it saying what it is. -->
      <div class="torrent-dir">
        <!-- The container is rtl so the ellipsis eats the head of the path
             rather than its tail — the meaningful part of a path is its last
             components — and `bdi` isolates the direction, or the slashes,
             being neutral characters, drift and "/Users/…" draws as "Users/…/". -->
        <span class="torrent-dir-path" title={torrentDir ?? ''}><bdi>{torrentDir ?? ''}</bdi></span>
        <!-- **Both actions belong to the field, so both sit in it.** This was a
             `.btn-outline` in `.link-actions` first — which is a dialog
             *footer*: full size, pushed to the bottom right, reading as the
             main action of the page rather than as an undo for one control.
             Beside the picker it is unmistakably about this path, and the pair
             needs no explanation of what it resets. One word for the same
             reason the delay stepper's reset is one: the noun is already in the
             label above, and a sentence here would squeeze the path.
             Quieter than the picker on purpose — choosing is the action, going
             back to the default is the correction. -->
        {#if !torrentDirDefault}
          <button
            class="torrent-dir-reset"
            onclick={async () => {
              await resetTorrentDir();
              await readTorrentDir();
            }}
          >
            {t('torrent.dir_reset')}
          </button>
        {/if}
        <button
          class="torrent-dir-pick"
          onclick={async () => {
            if (await pickTorrentDir()) await readTorrentDir();
          }}
        >
          {t('torrent.dir_change')}
        </button>
      </div>
      {#if torrentDirDefault}
        <div class="setting-hint">{t('torrent.dir_default_note')}</div>
      {/if}
    </div>

    <!-- Streaming a torrent writes the pieces to disk, so a few films fill
         a directory the viewer never chose to fill. -->
    <div class="setting">
      <div class="setting-label">{t('torrent.cache_clear')}</div>
      <div class="setting-hint">{t('torrent.cache_hint')}</div>
      <button class="btn-danger" onclick={onClearTorrentCache}>{t('torrent.cache_clear')}</button>
    </div>
  {:else if settingsTab === 'tv'}
    <div class="setting">
      <div class="setting-label">{t('cast.set_cache')}</div>
      <div class="segmented">
        {#each CAST_CAP_CHOICES as cap (cap)}
          <button
            class="segopt"
            class:sel={castCapVal === cap}
            onclick={() => setCastCapHere(cap)}
          >
            {cap === 0 ? t('cast.cache_none') : t('cast.cap_gb', { n: cap })}
          </button>
        {/each}
      </div>
      <div class="setting-hint">{t('cast.cache_hint')}</div>
      {#if castCacheBytes !== null}
        <div class="setting-hint">{t('cast.cache_size', { size: fmtSize(castCacheBytes) })}</div>
      {/if}
      <button class="btn-danger" onclick={() => void clearCastCacheHere()}>
        {t('cast.cache_clear')}
      </button>
    </div>
  {:else if settingsTab === 'playback'}
    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('set.queue')}</div>
          <div class="setting-hint">{t('set.queue_hint')}</div>
        </div>
        <button
          class="switch"
          class:on={playlist.queueFolder}
          role="switch"
          aria-checked={playlist.queueFolder}
          aria-label={t('set.queue')}
          onclick={() => setPlaylistPref('queueFolder', !playlist.queueFolder)}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    <div class="setting">
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('set.autoadvance')}</div>
          <div class="setting-hint">{t('set.autoadvance_hint')}</div>
        </div>
        <button
          class="switch"
          class:on={playlist.autoAdvance}
          role="switch"
          aria-checked={playlist.autoAdvance}
          aria-label={t('set.autoadvance')}
          onclick={() => setPlaylistPref('autoAdvance', !playlist.autoAdvance)}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

  {:else if settingsTab === 'keys'}
    {#each GROUP_ORDER as group (group)}
      <div class="keys-group">{t(`kgroup.${group}`)}</div>
      {#each ACTIONS.filter((a) => a.group === group) as def (def.id)}
        {@const chords = chordsOf(def.id)}
        <div class="setting keyrow">
          <div class="keyrow-name">{actionLabel(def.id)}</div>
          <div class="keyrow-chords">
            {#each chords as chord (chord)}
              <!-- A chord the macOS menu bar owns is shown but not
                   removable: taking it off here would change nothing,
                   since the system delivers it from the menu either way. -->
              <span class="kbadge" class:fixed={isMenuAccelerator(chord)}>
                {chordLabel(chord)}
                {#if !isMenuAccelerator(chord)}
                  <button
                    class="kbadge-x"
                    data-tip={t('keys.remove')}
                    aria-label={t('keys.remove')}
                    onclick={() => unassign(def.id, chord)}
                  >
                    <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
                  </button>
                {/if}
              </span>
            {/each}
            {#if IS_MAC && def.menuMac && !chords.includes(def.menuMac)}
              <span class="kbadge fixed" data-tip={t('keys.menu_note')}>
                {chordLabel(def.menuMac)}
              </span>
            {/if}
            {#if chords.length === 0 && !(IS_MAC && def.menuMac)}
              <span class="kbadge empty">{t('keys.none')}</span>
            {/if}
            {#if recordingAction === def.id}
              <span class="kbadge recording">{t('keys.press')}</span>
            {:else}
              <button
                class="kadd"
                data-tip={t('keys.add')}
                aria-label={t('keys.add')}
                onclick={() => startRecording(def.id)}
              >+</button>
            {/if}
            {#if isCustom(def.id)}
              <button class="settings-link kreset" onclick={() => { keyNote = null; resetAction(def.id); }}>
                {t('keys.restore')}
              </button>
            {/if}
          </div>
        </div>
        {#if recordingAction === def.id}
          <div class="setting-hint keynote">{t('keys.press_hint')}</div>
        {:else if keyNote?.id === def.id}
          <div class="setting-hint keynote">{keyNote.text}</div>
        {/if}
      {/each}
    {/each}
    <div class="settings-foot">{t('keys.footer')}</div>
    <button class="btn-danger" disabled={!hasCustomBindings()} onclick={() => { keyNote = null; resetAll(); }}>
      {t('keys.reset_all')}
    </button>

  {:else}
  {#each SETTINGS.filter((s) => s.tab === settingsTab) as s (s.key)}
    <div class="setting">
      <div class="setting-label">{s.label}</div>
      {#if s.kind === 'devices'}
        <div class="segmented vertical">
          {#each audioDevices as device (device.name)}
            <!-- mpv's own list starts with `auto`, and that is also its
                 default — mapped to null so choosing it CLEARS the
                 mpv.conf line instead of pinning "auto" in writing, the
                 same meaning "по умолчанию" has everywhere else here. -->
            {@const value = device.name === 'auto' ? null : device.name}
            <button
              class="segopt"
              class:sel={(settingsValues[s.key] ?? null) === value}
              onclick={() => setSetting(s, value)}
            >
              {device.description}
            </button>
          {/each}
        </div>
      {:else if s.kind === 'langs'}
        {@const codes = langsOf(s)}
        <!-- Chips in priority order, then the way to add one. The empty
             state says "авто" as a word rather than showing an empty
             strip, which reads as a control that failed to render. -->
        <div class="langs">
          {#if !codes.length}
            <span class="langs-auto">{t('vset.lang_auto')}</span>
          {/if}
          {#each codes as code, i (code)}
            <!-- Two buttons in a chip, never nested: a button inside a
                 button is invalid and the inner one stops being clicked.
                 The first chip is the preferred language and says so by
                 being filled, which is also why clicking it does nothing
                 — it is already what it would become. -->
            <span class="lang-chip" class:first={i === 0}>
              <button
                class="lang-name"
                data-tip={i === 0 ? t('vset.lang_primary') : t('vset.lang_promote')}
                disabled={i === 0}
                onclick={() => promoteLang(s, code)}
              >{languageName(code)}</button>
              <button
                class="lang-drop"
                aria-label={t('vset.lang_remove')}
                data-tip={t('vset.lang_remove')}
                onclick={() => removeLang(s, code)}
              >
                <svg viewBox="0 0 10 10" aria-hidden="true">
                  <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
                </svg>
              </button>
            </span>
          {/each}
          <button
            class="lang-add"
            class:open={langPickerFor === s.key}
            data-tip={t('vset.lang_add')}
            aria-label={t('vset.lang_add')}
            onclick={() => void openLangPicker(s)}
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M8 3.5v9M3.5 8h9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </div>
        {#if langPickerFor === s.key}
          <!-- Sixty languages behind a search field, which is the whole
               shape of this control: the common case is two clicks and the
               long tail is one word of typing away, without either being
               in the other's way. -->
          <div class="lang-picker">
            <input
              class="lang-search"
              type="text"
              spellcheck="false"
              autocapitalize="off"
              autocorrect="off"
              placeholder={t('vset.lang_search')}
              bind:this={langQueryEl}
              bind:value={langQuery}
              onkeydown={(e) => {
                // Enter takes the first hit — with a filtered list of one,
                // reaching for the mouse is the whole cost of the feature.
                if (e.key === 'Enter') {
                  e.preventDefault();
                  const hit = searchLanguages(langQuery, codes)[0];
                  if (hit) addLang(s, hit.code);
                }
                if (e.key === 'Escape') { e.preventDefault(); langPickerFor = null; }
                e.stopPropagation();
              }}
            />
            <div class="lang-list">
              {#each searchLanguages(langQuery, codes) as lang (lang.code)}
                <button class="lang-option" onclick={() => addLang(s, lang.code)}>
                  <span class="lang-option-name">{lang.name}</span>
                  <span class="lang-option-code">{lang.code}</span>
                </button>
              {:else}
                <div class="folders-empty">{t('vset.lang_none')}</div>
              {/each}
              <ScrollFade />
            </div>
          </div>
        {/if}
      {:else if s.kind === 'slider'}
        {@const value = sliderValue(s)}
        <div class="slider-row">
          <input
            type="range"
            class="setting-slider"
            min={s.min}
            max={s.max}
            step={s.step}
            {value}
            style="--progress: {((value - s.min) / (s.max - s.min)) * 100}%"
            aria-label={s.label}
            oninput={(e) => setSlider(s, Number(e.currentTarget.value))}
            onchange={(e) => e.currentTarget.blur()}
          />
          <span class="slider-value">{value.toFixed(s.decimals)}{s.suffix ?? ''}</span>
        </div>
      {:else}
        <div class="segmented">
          {#each s.options as o (o.label)}
            <button
              class="segopt"
              class:sel={settingsValues[s.key] === o.v}
              disabled={s.key === 'target-colorspace-hint' &&
                o.v !== null &&
                displayHdr !== null &&
                !displayHdr.supported}
              onclick={() => setSetting(s, o.v)}
            >
              {o.label}
            </button>
          {/each}
        </div>
      {/if}
      {#if s.key === 'target-colorspace-hint' && displayHdr}
        <div class="setting-hint">
          {#if !displayHdr.supported}
            {t('set.hdr_unsupported')}
          {:else if !displayHdr.enabled}
            {t(IS_MAC ? 'set.hdr_off_mac' : 'set.hdr_off_win')}
          {:else if settingsValues[s.key] === null}
            {t(IS_MAC ? 'set.hdr_forced_mac' : 'set.hdr_forced_win')}
          {:else}
            {t(IS_MAC ? 'set.hdr_on_mac' : 'set.hdr_on_win')}
          {/if}
        </div>
      {:else if s.hint}<div class="setting-hint">{s.hint}</div>{/if}
    </div>
  {/each}
    {#if settingsTab === 'audio'}
    <!-- Bitstream passthrough hands the receiver an undecoded stream, and a
         filter has nothing to attach to — so the switch says why it is
         unavailable rather than silently doing nothing. Kept by the player
         rather than written to mpv.conf (see the module), which is why it
         sits below the footer's claim about mpv's own settings. -->
    {@const bitstream = Boolean(settingsValues['audio-spdif'])}
    <div class="setting" class:muted={bitstream}>
      <div class="row-toggle">
        <div class="row-text">
          <div class="setting-label">{t('set.normalize')}</div>
          <div class="setting-hint">
            {t(bitstream ? 'set.normalize_spdif' : 'set.normalize_hint')}
          </div>
        </div>
        <button
          class="switch"
          class:on={player.normalize && !bitstream}
          role="switch"
          aria-checked={player.normalize && !bitstream}
          disabled={bitstream}
          aria-label={t('set.normalize')}
          onclick={() => applyNormalize(!player.normalize)}
        >
          <span class="switch-knob"></span>
        </button>
      </div>
    </div>

    {/if}
    {#if settingsTab === 'video'}
    {#if hwdecCurrent}
      <div class="settings-foot">
        {t('set.hwdec_foot')}
        {hwdecCurrent === 'no'
          ? t('set.hwdec_sw')
          : t('set.hwdec_hw', { name: hwdecCurrent })}
      </div>
    {/if}
    {/if}
    <div class="settings-foot">
      {t('set.conf_foot')}
      <button
        class="settings-link"
        disabled={!player.mpvConfPath}
        onclick={() => { if (player.mpvConfPath) void revealItemInDir(player.mpvConfPath); }}
      >
        {t(IS_MAC ? 'set.conf_reveal_mac' : 'set.conf_reveal_win')}
      </button>
    </div>
    <!-- The LGPL notice. The link opens the texts in a layer above this sheet
         rather than revealing a file: `open-path` is not in the capabilities,
         and a .md has no reliable handler on Windows regardless. Revealing the
         file is still offered, inside that dialog. -->
    <div class="settings-foot">
      {t('set.licenses_foot')}
      <button class="settings-link" onclick={onLicenses}>
        {t('set.licenses_open')}
      </button>
    </div>
  {/if}
</Dialog>

<style>
  /* ---- Toggle row ---- */


  .langs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
  }

  .langs-auto {
    padding: 4px 2px;
    color: #6a6a74;
    font-size: 12.5px;
  }

  /* The chip is the container and both controls sit inside it, because a button
     inside a button is invalid markup and the inner one stops receiving clicks.
     Its own background carries the state; neither child paints one. */
  .lang-chip {
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.1);
    transition: background 0.12s ease;
  }

  /* Indigo means one thing here as everywhere else — this is the selected one,
     the language mpv reaches for first. */
  .lang-chip.first {
    background: #6366f1;
  }

  .lang-name {
    padding: 5px 4px 5px 11px;
    border: none;
    background: transparent;
    color: #e0e0e6;
    font-size: 12.5px;
    cursor: pointer;
  }

  .lang-chip.first .lang-name {
    color: #fff;
  }

  /* The first chip is already what clicking it would make it, so it is inert —
     and says so by not changing under the pointer rather than by being dimmed,
     which would read as unavailable. */
  .lang-name:disabled {
    cursor: default;
  }

  .lang-drop {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-right: 4px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    /* Color, never opacity: this is a 1.4px stroke that is always on screen,
       and taking it off full opacity makes WKWebView re-rasterise and wobble it
       (the same finding as the torrent list's delete cross). */
    color: rgba(255, 255, 255, 0.4);
    cursor: pointer;
    transition: color 0.12s ease, background 0.12s ease;
  }

  .lang-chip.first .lang-drop {
    color: rgba(255, 255, 255, 0.62);
  }

  /* Written to win, not left to source order — measured in the built bundle,
     `.lang-chip.first .lang-drop` weighs (0,4,0) against a bare
     `.lang-drop:hover`'s (0,3,0), so on the preferred chip the cross would have
     stayed at its resting color under the pointer. The same arithmetic the
     link-history cross ran into. */
  .lang-chip .lang-drop:hover {
    background: rgba(0, 0, 0, 0.25);
    color: #fff;
  }

  .lang-drop svg {
    width: 9px;
    height: 9px;
  }

  /* 26 outer, border included — and it always was, because a <button> is
     border-box in both engines' own stylesheets, reset or no reset. */
  .lang-add {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 50%;
    background: transparent;
    color: #b9b9c3;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .lang-add svg {
    width: 13px;
    height: 13px;
  }

  .lang-picker {
    margin-top: 8px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }

  .lang-search {
    display: block;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    background: transparent;
    color: #e8e8ec;
    font-size: 12.5px;
  }

  .lang-search:focus {
    outline: none;
  }

  .lang-list {
    max-height: 176px;
    overflow-y: auto;
    /* Part of scrolling vertically rather than an extra: setting one axis
       computes the other from `visible` to `auto`. */
    overflow-x: hidden;
  }

  /* `width: 100%` is the row filling the list rather than overhanging it by its
     own 20px of padding, and that held before the global reset as well: this is
     a <button>, and both engines' stylesheets already make one border-box. The
     reset changes nothing here — worth knowing before "fixing" a row that was
     never broken. */
  .lang-option {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: #d0d0d8;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .lang-option:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .lang-option-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The code is here for the person who knows their file is tagged `jpn` and
     cannot read 日本語 — the same reason the search matches on codes. */
  .lang-option-code {
    flex: none;
    color: #6a6a74;
    font-size: 11px;
  }

  /* ---- Excluded folder list ---- */
  .folders {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 8px 7px 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
  }

  /* A field, and the same one `.link-input` is: same border, fill, radius and
     outer height, so the two read as one control shape across the sheet. The
     height is reproduced by the button plus this padding and border (28 + 8 +
     2) rather than by copying the input's padding, because here the button is
     what sets it — and every box is a border box (the reset in app.css), which
     is what makes the two numbers comparable at all.

     **Measured against the built stylesheet in both engines, and they do not
     agree about the input**: Chromium puts `.link-input` at 38.00 and WebKit at
     37.00, because its default line box for 13px text is a pixel shorter. So
     there is no height that matches on both, 38 matches Chromium exactly and
     WebKit by a pixel, and the two controls are never adjacent anyway — the
     relay field is in «Основные» and this is in «Раздачи». */
  .torrent-dir {
    display: flex;
    align-items: center;
    /* Tighter than the 8px it was: this gap now also falls *between* the two
       chips, and at 8px they read as two separate offers rather than as one
       pair of actions belonging to the path beside them. */
    gap: 6px;
    margin-top: 6px;
    /* Asymmetric on purpose: the right side is the gap around the button and
       obeys the concentric-corner rule below, while the left is `.link-input`'s
       own 12px, so text in the two fields starts at the same offset when they
       are stacked. */
    padding: 4px 4px 4px 12px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.06);
  }

  .torrent-dir-path {
    flex: 1;
    min-width: 0;
    /* Truncate the start, not the tail: for a path the meaningful part is its
       last components. Direction isolated by <bdi> in the markup. */
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #d6d6de;
    font-size: 12px;
  }

  /* Inner radius = outer minus the gap (8 − 4), the concentric rule a rounded
     box inside another rounded box has to obey or the two curves fight. */
  .torrent-dir-pick {
    flex: none;
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
    color: #d6d6de;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .torrent-dir-pick:hover {
    background: rgba(255, 255, 255, 0.14);
    color: #fff;
  }

  /* The picker's twin, one step quieter: same box, no fill until the pointer
     arrives. Two filled chips side by side would be two equal offers, and they
     are not — one chooses, the other undoes. */
  .torrent-dir-reset {
    flex: none;
    height: 28px;
    padding: 0 9px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #8a8a95;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .torrent-dir-reset:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #e8e8ec;
  }

  .folder-ico {
    flex: none;
    width: 15px;
    height: 15px;
    color: #8f8f9c;
  }

  .folder-path {
    flex: 1;
    min-width: 0;
    /* Truncate the start, not the tail: for paths the meaningful part is the
       last components. The text direction is isolated by <bdi> in the markup. */
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #d6d6de;
    font-size: 12px;
  }

  .folder-remove {
    flex: none;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .folder-remove svg {
    width: 9px;
    height: 9px;
    fill: none;
    display: block;
  }

  .folder-row:hover .folder-remove {
    color: #d6d6de;
  }

  .folder-remove:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  .folders-empty {
    padding: 7px 10px;
    color: #6a6a74;
    font-size: 12px;
  }

  .folder-add {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 8px 10px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .folder-add svg {
    width: 14px;
    height: 14px;
    fill: none;
  }

  .folder-add:hover:not(:disabled) {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.08);
    color: #e8e8ec;
  }

  .folder-add:disabled,
  .folder-remove:disabled {
    cursor: default;
  }

  .folder-remove:disabled:hover {
    background: transparent;
    color: #77777f;
  }

  /* ---- Key bindings editor ----
     A row is a flex pair rather than the label-above-control shape the rest of
     the dialog uses: thirty-six actions at two lines each is a page of
     scrolling, and a chord is short enough to sit beside its own name. */
  /* Its own class rather than a borrowed .menu-title. That one is a *menu*
     heading and carries `padding: 6px 10px` for it, which set every section
     title 10px right of the rows it labels — and the `padding: 0` written here
     lost, because the two selectors weigh the same and .menu-title is defined
     later in the file (measured in the shipped bundle: byte 22325 against
     27825). Exactly the trap .menu-item.chapter-item is on record for. Same
     look, no collision to lose. */
  .keys-group {
    /* 28 above against 8 below, and the asymmetry is the whole point: a heading
       has to bind to the section it labels far more strongly than to the one it
       follows, or thirty-six rows read as one undifferentiated list. The row
       above contributes nothing — adjoining sibling margins collapse, so the
       gap is this 28 and not 28 plus the row's 6. No :first-child case either:
       the heading is the dialog's third child (head, tabs, then this). */
    margin: 28px 0 8px;
    color: #9a9aa5;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }


  /* `min-width: 0` on the item AND on the row above it, or a long action name
     refuses to shrink below its content and the dialog grows a horizontal
     scrollbar instead of ellipsising — the same trap the queue rows hit. */
  .keyrow-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  .keyrow-chords {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 6px;
  }

  .kbadge {
    display: inline-flex;
    align-items: center;
    /* Measured against the shipped stylesheet: the glyph sits 9px from the
       chord text and 9px from the badge's padding box (10px from the border
       itself, which is the extra pixel of the border and reads as balanced).
       With the button flush against the text (gap: 0) its own 5px of internal
       padding was all that separated the two, against 8px on the outside — so
       the glyph read as pushed into the text it sat next to. */
    gap: 4px;
    padding: 3px 4px 3px 8px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    color: #e6e6ec;
    font-size: 12px;
    line-height: 1.3;
    white-space: nowrap;
  }

  /* Nothing to take off these two, so no × and no room held for one. */
  .kbadge.fixed,
  .kbadge.empty {
    padding-right: 8px;
    color: #77777f;
  }

  .kbadge.empty {
    border-style: dashed;
    background: none;
  }

  /* Indigo means one thing here as everywhere else: this is the live one. */
  .kbadge.recording {
    border-color: #6366f1;
    background: rgba(99, 102, 241, 0.18);
    color: #fff;
  }

  /* An 18px square, not a 7px glyph: the icon is the *mark*, the button is the
     target. As a bare <svg> the hit area was the strokes themselves, which on a
     badge this size is a few pixels of diagonal line — missable even when aimed
     at deliberately. The square is padding, so the glyph keeps its size. */
  .kbadge-x {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: none;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .kbadge-x svg {
    width: 8px;
    height: 8px;
    fill: none;
    stroke-width: 1.6;
  }

  /* Red on hover for the same reason .btn-danger is red: this one removes. */
  .kbadge-x:hover {
    background: rgba(248, 113, 113, 0.16);
    color: #f87171;
  }

  .kadd {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 6px;
    background: none;
    color: #9a9aa5;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .kadd:hover {
    border-color: #818cf8;
    border-style: solid;
    color: #fff;
  }

  .kreset {
    font-size: 11.5px;
  }

  /* Sits under the row it answers, not as a hint about the next one. */
  .keynote {
    margin: -2px 0 10px;
  }

  /* Inside the row the label is already offset by the toggle itself */


  /* A live readout, not a second sentence of the hint — so it gets air above
     it and reads brighter once the router has confirmed the mapping. Brighter
     rather than colored: the accent means selected/on/primary, and a passive
     readout wearing it inverts that (the OSD bar made exactly this mistake).
     Written as `.row-text .port-state` so it beats the `margin-top: 0` above,
     which is a descendant rule of the same shape and would otherwise win on
     source order. */
  .row-text .port-state {
    margin-top: 8px;
  }

  .port-state.ok {
    color: #b9b9c3;
  }

  /* A section that has lost its meaning under the current settings: visible
     but dimmed */
  .setting.muted {
    opacity: 0.45;
  }

  .lang-add:hover,
  .lang-add.open {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.1);
    color: #e8e8ec;
  }

  /* Same control, stacked: device names are phrases, and a row of pills would
     either overflow the dialog or truncate them to uselessness. */
  .segmented.vertical {
    flex-direction: column;
    gap: 1px;
  }

  .segmented.vertical .segopt {
    text-align: left;
    white-space: normal;
  }

  .setting.keyrow {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 28px;
    margin-bottom: 6px;
  }

  .setting-slider {
    flex: 1;
    min-width: 0;
  }
</style>

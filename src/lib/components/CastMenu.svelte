<script lang="ts">
  /// The television picker: one row per set, merged by IP address.
  ///
  /// Discovery runs only while this panel is up — mounting *is* opening, so the
  /// browse starts in an effect here and its cleanup covers every way the panel
  /// closes (Escape, an outside click, a cast starting). Nothing multicasts at
  /// startup, which is the rule that keeps the app from asking for Local
  /// Network permission before anyone has asked to cast.
  import {
    cast,
    castCurrentFile,
    castStateLabel,
    deviceProfile,
    deviceSummary,
    disconnectCast,
    pinnedUnavailable,
    plannedTransport,
    setDeviceTransport,
    startCastDiscovery,
    stopCastDiscovery,
    type TvDevice,
  } from '$lib/cast.svelte';
  import { t } from '$lib/i18n.svelte';
  import { IS_MAC } from '$lib/platform';

  interface Props {
    close: () => void;
    /// Which row is running a check. The report itself is a dialog the page
    /// owns, so the run is started from there.
    diagBusy: string | null;
    onDiagnose: (device: TvDevice) => void;
  }

  let { close, diagBusy, onDiagnose }: Props = $props();

  /// The search has been going long enough to say something about it. The panel
  /// must not announce failure while it is still looking: on macOS the system's
  /// Local Network prompt is usually still on screen at six seconds.
  let castSearchLong = $state(false);
  /// Which device row has its profile expanded. One at a time: two open rows in
  /// a panel this size read as a form rather than a list of televisions.
  let tvSettingsFor = $state<string | null>(null);

  $effect(() => {
    if (cast.active) return;
    startCastDiscovery();
    castSearchLong = false;
    tvSettingsFor = null;
    const timer = setTimeout(() => (castSearchLong = true), 6000);
    return () => {
      clearTimeout(timer);
      stopCastDiscovery();
    };
  });
</script>

<div class="menu castmenu scrollable">
  <div class="menu-title">{t('cast.title')}</div>
  {#if cast.active}
    <div class="cast-current">
      <span class="cast-name">{cast.deviceName}</span>
      <span class="cast-state">{castStateLabel()}</span>
    </div>
    <button
      class="menu-item"
      onclick={() => {
        close();
        void disconnectCast();
      }}
    >
      {t('cast.disconnect')}
    </button>
  {:else}
    {#each cast.tvs as device (device.key)}
      <!-- One row per television, however many protocols reach it. The
           second line states the consequence for the open file; the gear
           holds the per-device profile, which is where the protocol names
           live for whoever came looking for them. -->
      <div class="tv-row">
        <button
          class="menu-item cast-device"
          onclick={() => {
            close();
            void castCurrentFile(device);
          }}
        >
          <span class="cast-name">{device.name}</span>
          <span class="cast-model">
            {(cast.profileRevision, deviceSummary(device))}
          </span>
        </button>
        {#if cast.dlnaSweeping && !device.dlna}
          <div
            class="tv-sweep"
            data-tip={t('cast.looking_for_transports')}
            aria-label={t('cast.looking_for_transports')}
          ></div>
        {:else if device.cast && device.dlna}
          <button
            class="tv-gear"
            class:open={tvSettingsFor === device.key}
            aria-label={t('cast.transport_settings')}
            data-tip={t('cast.transport_settings')}
            onclick={() =>
              (tvSettingsFor = tvSettingsFor === device.key ? null : device.key)}
          >
            <svg
              viewBox="0 0 24 24"
              width="15"
              height="15"
              fill="none"
              stroke="currentColor"
              stroke-width="1.7"
              stroke-linecap="round"
              aria-hidden="true"
            >
              <path d="M4 8h8.4M17.6 8H20M4 16h4.4M13.6 16H20" />
              <circle cx="15" cy="8" r="2.6" />
              <circle cx="11" cy="16" r="2.6" />
            </svg>
          </button>
        {/if}
      </div>
      {#if tvSettingsFor === device.key}
        <!-- Expanded in place rather than in a second floating panel: a
             panel hoisted out of this one would need the submenu's hover
             bridge, its click guards and its drill-down fallback for a
             window too narrow to hold two — all of that for one control. -->
        <div class="tv-settings">
          <!-- Two blocks, in the order a person needs them: the choice
               with its explanation directly under it, then — behind a
               hairline, so it reads as a different subject — the way out
               when the choice is not the problem. The check used to sit
               between the control and its own hint, which split one
               thought in half. -->
          <div class="tv-block">
            <div class="tv-settings-label">{t('cast.transport')}</div>
            <div class="segmented">
              {#each [['auto', t('cast.transport_auto')], ['dlna', t('cast.transport_dlna')], ['cast', t('cast.transport_cast')]] as [value, label] (value)}
                <button
                  class="segopt"
                  class:sel={(cast.profileRevision, deviceProfile(device).transport === value)}
                  onclick={() => setDeviceTransport(device, value as 'auto' | 'cast' | 'dlna')}
                >
                  {label}
                </button>
              {/each}
            </div>
            <div class="tv-settings-hint">
              {#if pinnedUnavailable(device)}
                {t('cast.transport_unavailable')}
              {:else if deviceProfile(device).transport === 'auto'}
                {plannedTransport(device) === 'dlna'
                  ? t('cast.transport_auto_dlna')
                  : t('cast.transport_auto_cast')}
              {/if}
            </div>
          </div>
          <!-- The row says what the button is FOR. "Проверить устройство"
               on its own is a button whose purpose a viewer has to guess;
               the question in front of it is the whole affordance, and the
               action shrinks to a link-sized thing beside it. -->
          <div class="tv-block tv-trouble">
            <span class="tv-trouble-q">{t('cast.trouble')}</span>
            <button
              class="tv-check"
              disabled={diagBusy === device.key}
              onclick={() => onDiagnose(device)}
            >
              {diagBusy === device.key ? t('cast.diagnosing') : t('cast.diagnose')}
            </button>
          </div>
        </div>
      {/if}
    {:else}
      <!-- **Never a verdict while the search is still running.** The
           first version said "no devices found" after six seconds, which
           is exactly when a permission prompt is still on screen waiting
           to be answered — so the panel announced failure at the moment
           the viewer was in the middle of fixing it, and the only way
           forward was to close it and open it again. It now says what is
           true: still looking, and an allowed prompt will be picked up. -->
      <div class="cast-empty">
        <span class="cast-spin"></span>
        {castSearchLong ? t('cast.still_looking') : t('cast.searching')}
      </div>
    {/each}
    <!-- The number-one cause of "casting doesn't work" is the network,
         not the code, so the panel says so up front: the Defender prompt
         before the first cast, and the usual reasons a TV is invisible
         once the search has clearly come up dry. -->
    <!-- Three ages of the same panel. Before anything is known: the
         platform's permission warning, so the prompt is expected rather
         than a surprise. Once the search has run a while with nothing:
         what to do about it. Only after discovery has been rebuilt a few
         times — by then a granted prompt would have taken effect — the
         reasons a television is genuinely invisible. -->
    <div class="cast-hint">
      {#if cast.tvs.length}
        {IS_MAC ? t('cast.perm_warn_mac') : t('cast.firewall_warn')}
      {:else if cast.rebuilds >= 2}
        {t('cast.empty_hint')}
      {:else if castSearchLong}
        {IS_MAC ? t('cast.perm_wait_mac') : t('cast.perm_wait_win')}
      {:else}
        {IS_MAC ? t('cast.perm_warn_mac') : t('cast.firewall_warn')}
      {/if}
    </div>
  {/if}
</div>

<style>
  .cast-model {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.45);
  }

  .tv-row {
    display: flex;
    align-items: stretch;
  }

  .tv-row .cast-name,
  .tv-row .cast-model {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Same footprint as the gear, so nothing shifts when one replaces the other. */
  .tv-sweep {
    display: flex;
    align-items: center;
    align-self: center;
    width: 39px;
    height: 15px;
    justify-content: center;
  }

  .tv-sweep::after {
    content: '';
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid rgba(232, 232, 236, 0.25);
    border-top-color: rgba(232, 232, 236, 0.7);
    animation: spin 0.8s linear infinite;
  }

  .tv-gear {
    display: flex;
    align-items: center;
    padding: 0 12px;
    background: none;
    border: none;
    cursor: pointer;
    /* Three strengths in color, never opacity: a thin glyph re-rasterises
       when it leaves full opacity, and in WebKit that reads as a twitch. */
    color: rgba(232, 232, 236, 0.4);
  }

  .tv-row:hover .tv-gear {
    color: rgba(232, 232, 236, 0.66);
  }

  /* Deliberately not `.btn-outline`: that button is 15px with 20px of padding,
     which inside a 280px panel row reads as the panel's main action rather
     than as a way out of a rare problem. The question beside it carries the
     meaning; this only has to be pressable. */
  .tv-check {
    flex: none;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 6px;
    padding: 3px 10px;
    color: #d6d6de;
    font-size: 11px;
    cursor: pointer;
  }

  .tv-check:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.3);
    color: #e8e8ec;
  }

  .tv-check:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .cast-current {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 14px 8px;
    /* Explicit: this is a bare div, not a .menu-item button, so it inherits
       the document's default (black) rather than the menu's text color —
       which is exactly how the device name shipped unreadable. */
    color: #e8e8ec;
  }

  .cast-current .cast-name {
    font-weight: 600;
  }

  .cast-state {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.55);
  }

  .cast-spin {
    display: inline-block;
    vertical-align: -2px;
    width: 11px;
    height: 11px;
    margin-right: 7px;
    border-radius: 50%;
    border: 2px solid rgba(232, 232, 236, 0.25);
    border-top-color: rgba(232, 232, 236, 0.7);
    animation: spin 0.8s linear infinite;
  }

  .cast-empty {
    padding: 10px 14px;
    font-size: 12.5px;
    color: rgba(232, 232, 236, 0.55);
  }

  /* `.cast-hint` lives in app.css: the track menu says the same kind of thing
     about a DLNA session and was rendering it unstyled. */

  .tv-settings {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 14px 12px;
    color: #e8e8ec;
  }

  .tv-settings-label {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.55);
  }

  .tv-settings-hint {
    font-size: 11px;
    line-height: 1.4;
    color: rgba(232, 232, 236, 0.45);
  }

  /* The expanded row is two subjects, and the hairline is what says so: the
     choice with its explanation, then the way out when the choice is not the
     problem. */
  .tv-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  /* Written to win rather than left to source order: `.tv-block` sets a column
     and this element carries both classes, which is the two-class trap that has
     already cost this project two bugs. */
  .tv-block.tv-trouble {
    flex-direction: row;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  .tv-trouble-q {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.55);
  }

  /* Written to win against .menu-item's display: block (the two-class lesson:
     a lone modifier class of equal weight loses to source order). */
  .menu-item.cast-device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
  }

  /* Without this the long consequence line grows the row instead of
     ellipsizing, and the panel sprouts a horizontal scrollbar — min-width: auto
     is the flex default and every ancestor between the label and the fixed box
     needs it cleared. */
  .tv-row > .menu-item.cast-device {
    flex: 1;
    min-width: 0;
  }

  /* Written to beat the container's own hover: Svelte scopes the descendant
     selector with a free :where(), so `.tv-row:hover .tv-gear` outweighs a bare
     `.tv-gear:hover` and the glyph would never reach full strength. */
  .tv-row:hover .tv-gear:hover,
  .tv-gear.open {
    color: #e8e8ec;
  }
  .menu.castmenu {
    min-width: 280px;
  }
</style>

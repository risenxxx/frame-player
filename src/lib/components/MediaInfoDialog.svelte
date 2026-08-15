<script lang="ts">
  /// What the open file actually is: container, codecs, color, decoder — plus
  /// the swarm, when the bytes are arriving over BitTorrent.
  ///
  /// A live readout rather than a snapshot, which is why the refresh timer is
  /// owned here and not by whoever opens the panel: `video-bitrate` is a rolling
  /// average of recent packets, so it is simply absent for the first seconds of
  /// a file and drifts afterwards. Mounting *is* opening — the caller renders
  /// this component only while the panel is up, so the effect's lifetime is the
  /// panel's and nothing has to be torn down by hand.
  import Dialog from '$lib/components/Dialog.svelte';
  import { formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import {
    colorLine,
    isHdr,
    loadMediaInfo,
    overallBitrate,
    type MediaInfo,
  } from '$lib/mediainfo';
  import { player } from '$lib/player.svelte';
  import { stalls } from '$lib/stall.svelte';
  import { torrent } from '$lib/torrent.svelte';
  import { fmtFps, fmtRate, fmtSize, fmtSpeed } from '$lib/units';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let mediaInfo = $state<MediaInfo | null>(null);

  /// The largest desync seen since this file opened. A stall of the picture is
  /// a step in `avsync` that mpv then works off over the next few seconds, so
  /// the current value is almost always back to nothing by the time it is read.
  /// Only ever as good as the panel's own uptime, unlike the drop counters —
  /// which is exactly why those live in `player` and this does not: a peak is
  /// worth having when you are watching for one, an event has to be caught
  /// whether anybody was watching or not.
  let worstSync = $state(0);
  /// Refreshed with the readings, so "47 s ago" ticks with the panel rather
  /// than needing a clock of its own.
  let now = $state(Date.now());

  $effect(() => {
    void refreshInfo();
    const timer = setInterval(() => void refreshInfo(), 1000);
    return () => clearInterval(timer);
  });

  // The peak belongs to one file. Nothing else here is remembered across a file
  // change, so this must not be either.
  $effect(() => {
    void player.filePath;
    worstSync = 0;
  });

  async function refreshInfo() {
    if (!player.hasFile) {
      mediaInfo = null;
      return;
    }
    const info = await loadMediaInfo().catch(() => null);
    mediaInfo = info;
    now = Date.now();

    const avsync = info?.playback.avsync;
    if (avsync !== null && avsync !== undefined) worstSync = Math.max(worstSync, Math.abs(avsync));
  }

  /// The last time this process was stopped, and the worst it has been.
  ///
  /// "Nothing" is a real answer here rather than a missing value, and is said
  /// out loud: a quiet watchdog beside a drop counter that moved is what points
  /// at mpv rather than at the machine.
  function stallLine(): string {
    if (!stalls.last) return t('info.stall_none');
    const secs = Math.max(0, Math.round((now - stalls.last.at) / 1000));
    return t('info.stall_value', { ms: stalls.last.ms, secs, max: stalls.worst });
  }

  /// A drop counter and, when something has actually been lost, how long ago.
  function counter(drops: { count: number; last: { delta: number; at: number } | null }): string {
    if (!drops.last) return String(drops.count);
    const secs = Math.max(0, Math.round((now - drops.last.at) / 1000));
    return `${drops.count} · ${t('info.moved', { delta: drops.last.delta, secs })}`;
  }
</script>

<Dialog title={t('info.title')} scrollable {onclose}>
  {#if mediaInfo}
    {@const info = mediaInfo}
    <div class="info-section">{t('info.file')}</div>
    {@render infoRow(t('info.name'), player.filename)}
    {@render infoRow(t('info.container'), info.container?.toUpperCase() ?? null)}
    {@render infoRow(t('info.size'), info.size ? fmtSize(info.size) : null)}
    {@render infoRow(t('info.duration'), info.duration ? formatTime(info.duration) : null)}
    {@render infoRow(
      t('info.overall'),
      overallBitrate(info) ? fmtRate(overallBitrate(info)!) : null,
    )}

    <div class="info-section">{t('info.video')}</div>
    {@render infoRow(t('info.codec'), info.video.codec)}
    {@render infoRow(
      t('info.resolution'),
      info.video.width && info.video.height ? `${info.video.width} × ${info.video.height}` : null,
    )}
    {@render infoRow(t('info.fps_label'), info.video.fps ? fmtFps(info.video.fps) : null)}
    {@render infoRow(t('info.bitrate'), info.video.bitrate ? fmtRate(info.video.bitrate) : null)}
    {@render infoRow(t('info.pixfmt'), info.video.pixelFormat)}
    {@render infoRow(
      isHdr(info.video) ? t('info.hdr') : t('info.color'),
      colorLine(info.video),
    )}
    {@render infoRow(
      t('info.peak'),
      info.video.maxLuma ? t('info.nits', { value: Math.round(info.video.maxLuma) }) : null,
    )}
    {@render infoRow(
      t('info.decoding'),
      info.video.hwdec === 'no'
        ? t('info.decode_sw')
        : info.video.hwdec
          ? t('info.decode_hw', { name: info.video.hwdec })
          : null,
    )}

    <div class="info-section">{t('info.audio')}</div>
    {@render infoRow(t('info.codec'), info.audio.codec)}
    {@render infoRow(t('info.lang'), info.audio.lang)}
    {@render infoRow(t('info.channels'), info.audio.channels)}
    {@render infoRow(
      t('info.samplerate'),
      info.audio.sampleRate ? t('info.khz', { value: info.audio.sampleRate / 1000 }) : null,
    )}
    {@render infoRow(t('info.bitrate'), info.audio.bitrate ? fmtRate(info.audio.bitrate) : null)}

    <!-- How it is going, as opposed to what it is. Deliberately below the two
         track sections: this is a diagnosis and not part of identifying a
         file, and the rows only mean anything while something is playing. -->
    <div class="info-section">{t('info.playback')}</div>
    {@render infoRow(
      t('info.avsync'),
      info.playback.avsync === null
        ? null
        : t('info.avsync_value', {
            now: info.playback.avsync.toFixed(3),
            max: worstSync.toFixed(3),
          }),
    )}
    {@render infoRow(
      t('info.fps_actual'),
      info.playback.fpsActual ? fmtFps(info.playback.fpsActual) : null,
    )}
    {@render infoRow(t('info.drop_decoder'), counter(player.dropDecoder))}
    {@render infoRow(t('info.drop_vo'), counter(player.dropVo))}
    <!-- Read together with the two rows above, never on its own: which of them
         moved at the same moment is the whole diagnosis. -->
    {@render infoRow(t('info.stalls'), stallLine())}
    {@render infoRow(
      t('info.cache_ahead'),
      info.playback.cacheAhead === null
        ? null
        : t('info.seconds', { value: Math.round(info.playback.cacheAhead) }),
    )}

    <!-- Torrent state lives here rather than in chrome of its own: this
         panel already answers "what is this and how is it being played",
         and where the bytes are coming from is the same question. -->
    {#if torrent.status && torrent.status.state !== 'gone'}
      {@const ts = torrent.status}
      <div class="info-section">{t('torrent.info')}</div>
      {@render infoRow(t('torrent.info_peers'), String(ts.peers))}
      {@render infoRow(t('torrent.info_speed'), fmtSpeed(ts.down_bps))}
      {@render infoRow(
        t('torrent.info_downloaded'),
        ts.file_size ? `${fmtSize(ts.file_done)} / ${fmtSize(ts.file_size)}` : null,
      )}
    {/if}
  {/if}
</Dialog>

{#snippet infoRow(label: string, value: string | null)}
  <!-- A row with nothing in it is worse than a missing row: half of these
       properties are simply absent depending on the container, the codec and
       how long the file has been playing. -->
  {#if value}
    <div class="info-row">
      <span class="info-label">{label}</span>
      <span class="info-value">{value}</span>
    </div>
  {/if}
{/snippet}

<style>
  .info-section {
    margin: 16px 0 6px;
    color: #9a9aa5;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .info-section:first-of-type {
    margin-top: 4px;
  }

  .info-row {
    display: flex;
    gap: 16px;
    align-items: baseline;
    padding: 3px 0;
    font-size: 12.5px;
  }

  .info-label {
    flex: 0 0 38%;
    color: #8f8f9c;
  }

  /* Codec names and file names are long and arbitrary; the dialog has a fixed
     width, so this is the column that has to give. */
  .info-value {
    flex: 1;
    min-width: 0;
    color: #e0e0e6;
    overflow-wrap: anywhere;
  }
</style>

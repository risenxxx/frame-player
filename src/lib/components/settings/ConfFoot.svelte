<script lang="ts">
  /// The footer under the three mpv-backed tabs (Video, Audio, Subtitles): the
  /// one line that says where mpv's own settings live, and the report of an
  /// mpv.conf line mpv refused.
  import { revealItemInDir } from '@tauri-apps/plugin-opener';

  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import { player } from '$lib/player.svelte';
</script>

<!-- The durable half of the report. The popup at startup is easy to miss,
     and this is the one place a viewer comes to when a setting of theirs is
     not doing anything — with the file that caused it one click away. -->
{#if player.confError}
  <div class="settings-foot conf-error">
    {t('set.conf_ignored')}
    <span class="conf-error-detail">{player.confError}</span>
  </div>
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

<style>
  /* Written to beat `.settings-foot` in app.css rather than left to whichever
     stylesheet lands later — two bugs in this project came from a lone modifier
     class of equal weight losing that race. */
  .settings-foot.conf-error {
    color: #f87171;
  }

  /* mpv's own words, which name the option. Its own line and its own colour:
     it is evidence to read, not part of the sentence above it. */
  .conf-error-detail {
    display: block;
    margin-top: 4px;
    color: #9a9aa5;
    overflow-wrap: anywhere;
  }
</style>

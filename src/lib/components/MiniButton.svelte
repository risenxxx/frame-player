<script lang="ts">
  /// The way out of the mini player, in the window's corner. Hidden unless the
  /// player is mini, and fading with the rest of the interface there — both
  /// read off the page's root (`.player.mini`, `.idle-ui`), so those two rules
  /// are written against that ancestor with `:global`, which keeps the markup
  /// exactly what it was.
  import { t } from '$lib/i18n.svelte';
  import { withKey } from '$lib/keys.svelte';
  import { toggleMini } from '$lib/window-prefs.svelte';
</script>

<button
  class="minibtn"
  data-tip={withKey(t('ctx.mini_exit'), 'mini')}
  aria-label={t('ctx.mini_exit')}
  onclick={(e) => {
    e.stopPropagation();
    void toggleMini();
  }}
>
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/>
  </svg>
</button>

<style>
  /* The way out has to be visible in the mode that hides everything else. */
  :global(.player.mini) .minibtn {
    display: grid;
  }

  /* Fades with the rest of the UI: in a corner window the picture is the point,
     and a button pinned over it forever is one more thing in the way. */
  :global(.player.mini.idle-ui) .minibtn {
    opacity: 0;
    pointer-events: none;
  }

  /* Round, and that is the whole reason: a rounded rectangle 8px inside the
     window's own rounded corner has to obey the concentric rule (inner radius
     = outer minus the gap) to look right, and the outer radius is not ours to
     know — macOS decides it, Windows 11 decides a different one, and both can
     change with the OS. A circle has no corner to disagree with, which is why
     the system's own corner controls (the traffic lights) are circles too. */
  .minibtn {
    display: none;
    position: absolute;
    top: 12px;
    right: 12px;
    z-index: 20;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 999px;
    background: rgba(16, 16, 22, 0.82);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #d6d6de;
    cursor: pointer;
    transition: opacity 0.25s ease;
  }

  .minibtn svg {
    width: 14px;
    height: 14px;
  }

  .minibtn:hover {
    background: rgba(32, 32, 42, 0.9);
    color: #fff;
  }
</style>

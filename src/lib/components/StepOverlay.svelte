<script lang="ts">
  /// The frame-stepper's canvas. The sidecar StepEngine is off by default
  /// (`USE_STEP_ENGINE`), so this is normally an empty, invisible layer.
  import { formatTimeMs } from '$lib/format';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    visible: boolean;
    pts: number;
    canvas: HTMLCanvasElement | null;
  }

  let { visible, pts, canvas = $bindable() }: Props = $props();
</script>

<div class="stepwrap" class:visible>
  <canvas bind:this={canvas}></canvas>
  <div class="stepbadge">{t('step.badge')} · {formatTimeMs(pts)}</div>
</div>

<style>
  .stepwrap {
    position: absolute;
    inset: 0;
    display: none;
    place-items: center;
    background: #000;
    pointer-events: none;
  }

  .stepwrap.visible {
    display: grid;
  }

  .stepwrap canvas {
    max-width: 100%;
    max-height: 100%;
  }

  .stepbadge {
    position: absolute;
    bottom: 128px;
    left: 16px;
    background: rgba(16, 16, 22, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    color: #e8e8ec;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    padding: 5px 10px;
  }
</style>

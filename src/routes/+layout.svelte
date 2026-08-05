<script lang="ts">
  // Exists only to pull in the document-level stylesheet; both windows (player
  // and veil shutter) go through here.
  import '../app.css';

  let { children } = $props();
</script>

{@render children()}

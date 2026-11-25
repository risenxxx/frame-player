// Files dropped from Finder or Explorer, as Tauri delivers them. The player listens through
// `getCurrentWebview().onDragDropEvent` (src/lib/boot.ts), which is four Tauri events:
// `tauri://drag-enter` `{ paths, position }`, `tauri://drag-over` `{ position }`,
// `tauri://drag-drop` `{ paths, position }` and `tauri://drag-leave` (no payload), with `position`
// in physical px relative to the page (@tauri-apps/api 2.x, webview.js). Skathi delivers a native
// drop as DOM drag events instead (Skathi's docs/pages/guides/file-drop.md): `preventDefault` on
// `dragover` accepts it, and `appFiles.getPathForFile` turns a dropped `File`, entry or handle into
// its absolute path. Nothing here imports Skathi or Tauri, so the mapping is testable under node.

/** Where the bridge sends Tauri's events (the stub passes `emit` from `@tauri-apps/api/event`). */
export type Emit = (event: string, payload?: unknown) => unknown;
export type PathOf = (fileOrEntry: object) => string | null;

/** The part of `DataTransfer` the bridge reads. */
interface Transfer {
  types: ArrayLike<string> | readonly string[];
  files: ArrayLike<File>;
  items?: ArrayLike<{ kind: string; webkitGetAsEntry?: () => unknown }>;
  dropEffect: string;
}

/**
 * The absolute paths of a drop, in its order. A directory arrives as a zero-length placeholder in
 * `files`; its path is asked of the placeholder first and then of its entry, which Skathi's own
 * tests show answering with the folder's path. The folder goes to the player as it is: v1's
 * `loadFiles` hands any path that is not a subtitle, audio or torrent to mpv's `loadfile`, and mpv
 * opens a folder as a playlist of what is in it — the same as a folder dropped on version 1.
 * Files with no path (made by a script, or the virtual root) are skipped.
 */
export function dropPaths(dt: Transfer, pathOf: PathOf): string[] {
  const out: string[] = [];
  const items = dt.items;
  for (let i = 0; i < dt.files.length; i++) {
    let p: string | null = null;
    try {
      p = pathOf(dt.files[i]);
    } catch {
      /* not a host file */
    }
    if (!p && items && items[i]?.kind === 'file') {
      try {
        const entry = items[i].webkitGetAsEntry?.();
        if (entry) p = pathOf(entry as object);
      } catch {
        /* no entry */
      }
    }
    if (p) out.push(p);
  }
  return out;
}

const hasFiles = (dt: Transfer | null | undefined): dt is Transfer =>
  !!dt && Array.from(dt.types as ArrayLike<string>).includes('Files');

/**
 * An element that takes a drop itself: a file input (it accepts drops by default) or a text field.
 * The bridge leaves those to the browser behaviour. Nothing in version 1 has one today.
 */
function ownsDrop(t: EventTarget | null): boolean {
  const el = t as Element | null;
  if (!el || typeof el.closest !== 'function') return false;
  return !!el.closest('input, textarea, [contenteditable=""], [contenteditable="true"]');
}

/**
 * Installs the bridge on `target` (the document). Enter, over and leave are watched in the capture
 * phase, before anything in the page: a drag of files is accepted everywhere, as the player's web
 * view accepted it anywhere in the window. The drop itself is taken in the bubble phase on the same
 * target, so a component that handles a drop and calls `preventDefault` (or stops it) keeps it.
 *
 * Enter and leave fire for every element the pointer crosses; one Tauri `enter` is sent on the
 * first, and one `leave` when the count of entered elements is back to zero, so moving over the
 * page's children does not flicker. Returns the function that removes the bridge.
 */
export function installFileDrop(
  target: EventTarget,
  emit: Emit,
  pathOf: PathOf,
  scale: () => number,
): () => void {
  let depth = 0;
  const position = (e: DragEvent) => {
    const k = scale() || 1;
    return { x: Math.round(e.clientX * k), y: Math.round(e.clientY * k) };
  };
  const onEnter = (e: Event) => {
    const d = e as DragEvent;
    if (!hasFiles(d.dataTransfer)) return;
    if (depth++ === 0) {
      // Paths are not readable before the drop on Skathi (its guide), so `enter` carries none.
      void emit('tauri://drag-enter', { paths: [], position: position(d) });
    }
  };
  const onOver = (e: Event) => {
    const d = e as DragEvent;
    if (!hasFiles(d.dataTransfer) || ownsDrop(d.target)) return;
    d.preventDefault();
    d.dataTransfer.dropEffect = 'copy';
    void emit('tauri://drag-over', { position: position(d) });
  };
  const onLeave = (e: Event) => {
    const d = e as DragEvent;
    if (!hasFiles(d.dataTransfer) || depth === 0) return;
    if (--depth === 0) void emit('tauri://drag-leave', null);
  };
  // The count is reset in the capture phase, so a component that stops the drop does not leave
  // it standing for the next drag.
  let had = false;
  const onDropCapture = () => {
    had = depth > 0;
    depth = 0;
  };
  const onDrop = (e: Event) => {
    const d = e as DragEvent;
    if (!hasFiles(d.dataTransfer)) return;
    if (d.defaultPrevented || ownsDrop(d.target)) {
      // Taken by the page: the drag is over all the same.
      if (had) void emit('tauri://drag-leave', null);
      return;
    }
    d.preventDefault();
    void emit('tauri://drag-drop', { paths: dropPaths(d.dataTransfer, pathOf), position: position(d) });
  };
  target.addEventListener('dragenter', onEnter, true);
  target.addEventListener('dragover', onOver, true);
  target.addEventListener('dragleave', onLeave, true);
  target.addEventListener('drop', onDropCapture, true);
  target.addEventListener('drop', onDrop, false);
  return () => {
    target.removeEventListener('dragenter', onEnter, true);
    target.removeEventListener('dragover', onOver, true);
    target.removeEventListener('dragleave', onLeave, true);
    target.removeEventListener('drop', onDropCapture, true);
    target.removeEventListener('drop', onDrop, false);
  };
}

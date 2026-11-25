import { describe, expect, it } from 'vitest';
import { dropPaths, installFileDrop } from './file-drop';

/** A drag event as Skathi dispatches one, on a bare EventTarget (node has no DOM). */
function drag(type: string, dt: object, at = { x: 10, y: 20 }): Event {
  const e = new Event(type, { cancelable: true });
  Object.assign(e, { dataTransfer: dt, clientX: at.x, clientY: at.y });
  return e;
}

const file = (name: string) => ({ name }) as unknown as File;
const PATHS: Record<string, string> = { 'clip.mp4': '/m/clip.mp4', 'clip.srt': '/m/clip.srt' };
const pathOf = (f: object) => PATHS[(f as { name: string }).name] ?? null;

function transfer(names: string[], items?: { kind: string; webkitGetAsEntry?: () => unknown }[]) {
  return { types: ['Files'], files: names.map(file), items, dropEffect: 'none' };
}

describe('dropPaths', () => {
  it('maps files to absolute paths in order and skips those without one', () => {
    expect(dropPaths(transfer(['clip.mp4', 'made-by-script', 'clip.srt']), pathOf)).toEqual(['/m/clip.mp4', '/m/clip.srt']);
  });

  it("asks a folder's entry when its placeholder file has no path", () => {
    const dt = transfer(['clips'], [{ kind: 'file', webkitGetAsEntry: () => ({ name: 'entry:clips' }) }]);
    const of = (f: object) => ((f as { name: string }).name === 'entry:clips' ? '/m/clips' : null);
    expect(dropPaths(dt, of)).toEqual(['/m/clips']);
  });

  it('survives a host that throws', () => {
    expect(dropPaths(transfer(['clip.mp4']), () => { throw new Error('no host'); })).toEqual([]);
  });
});

describe('installFileDrop', () => {
  function setup(scale = 2) {
    const target = new EventTarget();
    const sent: [string, unknown][] = [];
    const off = installFileDrop(target, (ev, p) => sent.push([ev, p]), pathOf, () => scale);
    return { target, sent, off };
  }

  it("sends Tauri's events with paths and physical positions, and accepts the drop", () => {
    const { target, sent } = setup();
    const dt = transfer(['clip.mp4']);
    target.dispatchEvent(drag('dragenter', dt));
    const over = drag('dragover', dt);
    target.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(true);
    expect(dt.dropEffect).toBe('copy');
    const drop = drag('drop', dt, { x: 5.4, y: 7 });
    target.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(true);
    expect(sent).toEqual([
      ['tauri://drag-enter', { paths: [], position: { x: 20, y: 40 } }],
      ['tauri://drag-over', { position: { x: 20, y: 40 } }],
      ['tauri://drag-drop', { paths: ['/m/clip.mp4'], position: { x: 11, y: 14 } }],
    ]);
  });

  it('sends one enter and one leave however many children the pointer crosses', () => {
    const { target, sent } = setup();
    const dt = transfer(['clip.mp4']);
    // Entering a child fires its dragenter before the parent's dragleave.
    for (const t of ['dragenter', 'dragenter', 'dragleave', 'dragenter', 'dragleave', 'dragleave']) target.dispatchEvent(drag(t, dt));
    expect(sent.map(([ev]) => ev)).toEqual(['tauri://drag-enter', 'tauri://drag-leave']);
  });

  it('ignores drags that carry no files, and stops after removal', () => {
    const { target, sent, off } = setup();
    const text = { types: ['text/plain'], files: [], dropEffect: 'none' };
    const over = drag('dragover', text);
    target.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(false);
    off();
    target.dispatchEvent(drag('drop', transfer(['clip.mp4'])));
    expect(sent).toEqual([]);
  });

  it('leaves a drop the page already took', () => {
    const { target, sent } = setup();
    const dt = transfer(['clip.mp4']);
    target.dispatchEvent(drag('dragenter', dt));
    // A handler registered on the target itself, ahead of the bridge's bubble listener.
    const taken = drag('drop', dt);
    taken.preventDefault();
    target.dispatchEvent(taken);
    expect(sent.map(([ev]) => ev)).toEqual(['tauri://drag-enter', 'tauri://drag-leave']);
  });
});

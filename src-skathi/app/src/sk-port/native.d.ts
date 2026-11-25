declare module 'native:mpv' {
  export type Value = null | boolean | number | string | Value[] | { [key: string]: Value };
  export function init(options: [string, Value][], observed: [string, string][]): void;
  export function command(name: string, args: Value[]): void;
  export function setProperty(name: string, value: Value): void;
  export function getProperty(name: string, format: string): Value;
  export function destroy(): void;
  export function takePendingFiles(): string[];
  export function openDialog(multiple: boolean, directory: boolean): Promise<string[]>;
  /** The directory Tauri's `resourceDir()` names: lua/, shaders/, the notices. */
  /** The player's small commands (src/commands.rs), under their Tauri names in camelCase. */
  export function folderEntries(path: string): string[];
  export function pathsExist(paths: string[]): boolean[];
  export function externalTrackDirs(path: string): Promise<{ subs: string[]; audio: string[] }>;
  export function userMpvConf(): { path: string; options: [string, string][] };
  export function mpvConfSet(key: string, value: string | null): void;
  export function thirdPartyNotices(): string;
  export function screenshotDir(): string;
  export function screenshotClipPath(): string;
  export function screenshotToClipboard(path: string): Promise<void>;
  export function trashFile(path: string): void;
  export function primaryButtonDown(): boolean;
  /** Windows: the system's sizing loop for one edge (`North`, `SouthEast`, …); false when none started. */
  export function startResize(direction: string): boolean;
  export function doubleClickTime(): number;
  export function debugLogState(): { verbose: boolean; since: number | null; from_env: boolean };
  export function debugLogSet(on: boolean): { verbose: boolean; since: number | null; from_env: boolean };
  export function zoomPan(z: number, x: number, y: number): void;
  export function mpvShaderCacheDir(): string;
  /** Now Playing (now_playing.rs): handlers once, then the listing; commands arrive on `remote`. */
  export function nowPlayingStart(): void;
  export function nowPlayingSet(info: unknown): void;
  export function subtitleFontsDir(): string;
  export function subtitleFontsList(): Promise<unknown[]>;
  export function subtitleFontAdd(path: string): Promise<unknown>;
  export function subtitleFontRemove(name: string): void;
  export function hidden(): boolean;
  export function resourceDir(): string;
  /** The host's data directory: settings files, caches, screenshots. */
  export function dataDir(): string;
  /** Whether mpv draws into a native layer under the page (the page then asks for a transparent backdrop). */
  export function layerRoute(): boolean;
  /** Windows, FPUI_FS_ANIM (on by default; `0` turns it off): the fullscreen animation's length in ms, 0 when off (macOS has none). */
  export function fsAnimMs(): number;
  /** macOS only: the fullscreen state the page wants; the host applies it now or after the transition under way. */
  export function fsWant(on: boolean): void;
  export function fsAnimEnter(ms: number): boolean;
  export function fsAnimExit(ms: number): boolean;
  /** thumb_service.rs (src/thumbs.rs). The bytes are [f64 key LE][JPEG], as the player's raw Response. */
  export function thumbStart(path: string, duration: number): Promise<void>;
  export function thumbGet(path: string, pos: number, exact: boolean): Promise<Uint8Array>;
  export function setPrivatePaths(paths: string[], all: boolean): void;
  export function forgetThumbs(path: string): void;
  export function forgetThumbsUnder(folder: string): Promise<number>;
  export function clearThumbCache(): Promise<void>;
  export function posterFrame(path: string, pos: number): Promise<Uint8Array>;
  export function posterSaved(id: string): Uint8Array;
  export function posterCapture(id: string, path: string, positions: number[]): Promise<boolean>;
  export function cropDetect(path: string, positions: number[] | null): Promise<{ x: number; y: number; w: number; h: number } | null>;
  export function containerTitles(paths: string[]): Promise<(string | null)[]>;
  export function on(event: 'event', handler: (event: { event: string } & Record<string, Value>) => void): () => void;
  /** A Now Playing command (pause, play, toggle, next, previous, seek with `position`). */
  export function on(event: 'remote', handler: (command: { command: string; position?: number }) => void): () => void;
  /** Windows: subtitles or the OSD reach above the overlay band, so the overlay takes the whole video. */
  export function on(event: 'overlay', handler: (payload: { full?: boolean }) => void): () => void;
  /** macOS: the green button or AppKit's menu asked for fullscreen in or out. */
  export function on(event: 'fullscreen', handler: (payload: unknown) => void): () => void;
  /** macOS: a live resize by the pointer began (`on`) or ended. */
  export function on(event: 'resizing', handler: (payload: { on?: boolean }) => void): () => void;
  /** macOS, FPUI_OVL_BAND=auto: the fraction of the height the overlay band should take. */
  export function on(event: 'band', handler: (band: { f?: number }) => void): () => void;
}

/** Version 1's cast discovery (src/cast_module.rs): the device lists only, nothing past them. */
declare module 'native:cast' {
  export function castDiscoverStart(hints: string[]): void;
  export function castDiscoverStop(): void;
  export function castDevices(): unknown[];
  export function dlnaDiscoverStart(hints: string[]): void;
  export function dlnaDiscoverStop(): void;
  export function dlnaDevices(): unknown[];
}

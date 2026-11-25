// A stand-in for `native:mpv` when the payload runs on a stock Skathi workload (no embedder, so no
// native modules): every call answers what "no mpv, nothing on disk" would. It exists so the same
// payload can be measured on Windows under the JIT, AOT and the aot-nojit workload, none of which an
// embedder of the `skathi` crate can select. No video, no events: the start screen and the menus.
type Value = unknown;
type State = { verbose: boolean; since: number | null; from_env: boolean };

const none = <T>(v: T) => () => v;
const fail = (what: string) => () => Promise.reject(new Error(`${what}: no native module on this host`));
const DEBUG: State = { verbose: false, since: null, from_env: false };

export const fake = {
  init(_options: [string, Value][], _observed: [string, string][]) {},
  command(_name: string, _args: Value[]) {},
  setProperty(_name: string, _value: Value) {},
  getProperty(_name: string, format: string): Value {
    if (format === 'flag') return false;
    if (format === 'int64' || format === 'double') return 0;
    return '';
  },
  destroy() {},
  takePendingFiles: none<string[]>([]),
  openDialog: (_m: boolean, _d: boolean) => Promise.resolve<string[]>([]),
  folderEntries: (_p: string): string[] => [],
  pathsExist: (paths: string[]) => paths.map(() => false),
  externalTrackDirs: (_p: string) => Promise.resolve({ subs: [] as string[], audio: [] as string[] }),
  userMpvConf: () => ({ path: '', options: [] as [string, string][] }),
  mpvConfSet(_k: string, _v: string | null) {},
  thirdPartyNotices: none(''),
  screenshotDir: none(''),
  screenshotClipPath: none(''),
  screenshotToClipboard: fail('screenshot_to_clipboard'),
  trashFile(_p: string) { throw new Error('trash_file: no native module on this host'); },
  primaryButtonDown: none(false),
  startResize: (_d: string) => false,
  doubleClickTime: none(500),
  debugLogState: () => DEBUG,
  debugLogSet: (_on: boolean) => DEBUG,
  zoomPan(_z: number, _x: number, _y: number) {},
  mpvShaderCacheDir: none(''),
  nowPlayingStart() {},
  nowPlayingSet(_info: unknown) {},
  subtitleFontsDir: none(''),
  subtitleFontsList: () => Promise.resolve<unknown[]>([]),
  subtitleFontAdd: fail('subtitle_font_add'),
  subtitleFontRemove(_n: string) {},
  hidden: none(false),
  resourceDir: none(''),
  dataDir: none(''),
  layerRoute: none(false),
  fsAnimMs: none(0),
  fsAnimEnter: (_ms: number) => false,
  fsWant: (_on: boolean) => {},
  fsAnimExit: (_ms: number) => false,
  thumbStart: (_p: string, _d: number) => Promise.resolve(),
  thumbGet: fail('thumb_get') as (p: string, pos: number, exact: boolean) => Promise<Uint8Array>,
  setPrivatePaths(_p: string[], _all: boolean) {},
  forgetThumbs(_p: string) {},
  forgetThumbsUnder: (_f: string) => Promise.resolve(0),
  clearThumbCache: () => Promise.resolve(),
  posterFrame: fail('poster_frame') as (p: string, pos: number) => Promise<Uint8Array>,
  posterSaved(_id: string): Uint8Array { throw new Error('poster_saved: no native module on this host'); },
  posterCapture: (_id: string, _p: string, _pos: number[]) => Promise.resolve(false),
  cropDetect: (_p: string, _pos: number[] | null) => Promise.resolve(null),
  containerTitles: (paths: string[]) => Promise.resolve(paths.map(() => null)),
  on: (_event: string, _handler: (e: never) => void) => () => {},
};

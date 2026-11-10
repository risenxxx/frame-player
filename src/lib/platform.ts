/** Which desktop we are on. A user-agent sniff because the webview is the only
 *  thing that knows without an IPC round trip, and this is read during init.
 *
 *  Three answers, and the third is not "Windows": a lot of the code was written
 *  when `!IS_MAC` meant Windows, so a check that is really about Windows — the
 *  registry, the in-place update, Explorer, the Recycle Bin — says `IS_WINDOWS`,
 *  and Linux takes whichever branch is true of it. WebKitGTK's user agent says
 *  `Linux` (and `X11` under XWayland); WebView2's says `Windows`. */
export const IS_MAC = /Mac|iPhone|iPad/.test(navigator.userAgent);
export const IS_WINDOWS = /Windows/.test(navigator.userAgent);
export const IS_LINUX = !IS_MAC && !IS_WINDOWS && /Linux|X11/.test(navigator.userAgent);

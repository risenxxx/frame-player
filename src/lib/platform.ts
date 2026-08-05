/** Which desktop we are on. A user-agent sniff because the webview is the only
 *  thing that knows without an IPC round trip, and this is read during init. */
export const IS_MAC = /Mac|iPhone|iPad/.test(navigator.userAgent);

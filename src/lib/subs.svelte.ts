/**
 * Subtitle search and download (OpenSubtitles).
 *
 * Two things shape this panel. A hash match and a title match are *not* the
 * same result — the first is the same release and drops in already in sync, the
 * second is a guess about which rip you have — so which one produced the list is
 * said out loud rather than left for the viewer to discover by watching lips.
 * And the hash is only sent for a file outside the private folders: it names
 * what is being watched as surely as a stored position does. There the panel
 * searches by typed title instead, which is the viewer's own words rather than
 * something we volunteered.
 *
 * The state lives here rather than in the panel because two other places reach
 * into it: the track menu removes a subtitle, and the page opens the panel.
 */

import { invoke } from '@tauri-apps/api/core';
import { tick } from 'svelte';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { parseEpisode, type EpisodeRef } from '$lib/format';
import { forgetDownloadedSub, isDownloadedSub, rememberDownloadedSub } from '$lib/history.svelte';
import { locale, t } from '$lib/i18n.svelte';
import { languageName, parseLangList } from '$lib/languages';
import { showOsd } from '$lib/osd.svelte';
import { attachTrack, player, removeSubTrack, type Track } from '$lib/player.svelte';

export type SubHit = {
  file_id: number;
  file_name: string;
  language: string;
  release: string;
  movie: string;
  year: number | null;
  parent: string;
  season: number | null;
  episode: number | null;
  downloads: number;
  hash_match: boolean;
  hearing_impaired: boolean;
  from_trusted: boolean;
  ai_translated: boolean;
  fps: number | null;
  uploader: string;
};

export type SubsAccount = {
  signed_in: boolean;
  username: string | null;
  remembered_password: boolean;
  allowed_downloads: number | null;
  remaining_downloads: number | null;
  level: string | null;
  keychain_failed: boolean;
};

/**
 * Registration. The locale prefix follows the pattern the API documentation
 * uses for the consumers page (`/en/consumers`); the path itself is Devise's
 * convention and could not be verified from here — the site sits behind
 * Cloudflare, which answers every programmatic request with 403, including for
 * pages known to exist.
 */
export const OPENSUBTITLES_SIGNUP = 'https://www.opensubtitles.com/en/users/sign_up';

class Subs {
  open = $state(false);
  query = $state('');
  lang = $state('');
  busy = $state(false);
  error = $state<string | null>(null);
  hits = $state<SubHit[] | null>(null);
  matchKind = $state<'hash' | 'title'>('title');
  /// The file's hash was looked up and matched nothing — the useful negative,
  /// and the difference between "here is a guess" and "OpenSubtitles has
  /// nothing for this release".
  hashTried = $state(false);
  hashBlocked = $state(false);
  quota = $state<{ remaining: number | null; reset: string | null } | null>(null);
  busyId = $state<number | null>(null);

  /// The subtitle languages this viewer has asked for, read from mpv when the
  /// panel opens (see `openSubsDialog`).
  langPrefs = $state<string[]>([]);

  /// The video's own frame rate, read once when the panel opens.
  ///
  /// OpenSubtitles carries **no duration** for a subtitle — measured, there is
  /// no such field anywhere in a search result, in `files[]`, in
  /// `feature_details` or in the feature record — so the question "will this one
  /// fit" has to be answered by what there is. `fps` is it: a 25 fps subtitle on
  /// a 23.976 fps file does not merely start late, it drifts further out the
  /// longer you watch, which is the failure people actually hit and cannot
  /// diagnose.
  videoFps = $state<number | null>(null);

  /// Season and episode read off the file name, when it says so. Sent with a
  /// title search and shown in the panel, because otherwise a list of episodes
  /// appearing for a one-word query looks like magic.
  episode = $state<EpisodeRef | null>(null);

  // ---- The OpenSubtitles account ----------------------------------------
  //
  // Signing in is an upgrade, not a gate: downloading works signed out (5 a day
  // per IP), and an account raises that to 10–1000. So the form is folded away
  // until asked for, and the panel never blocks on it.

  account = $state<SubsAccount | null>(null);
  authOpen = $state(false);
  user = $state('');
  pass = $state('');
  remember = $state(false);
  authBusy = $state(false);
  authError = $state<string | null>(null);

  /// DOM handles the panel binds with `bind:this`. Deliberately not `$state`:
  /// nothing renders from them, they are only read inside the callbacks below.
  inputEl: HTMLInputElement | null = null;
  authEl: HTMLDivElement | null = null;
  authUserEl: HTMLInputElement | null = null;

  /**
   * The language filter, which is the viewer's preferences plus "any".
   *
   * It was a hardcoded Russian/English/any, which is the same leak ROADMAP 25
   * is about: those are the two languages the *interface* is translated into,
   * and they had become the only ones a search could filter by. Deriving it
   * costs no second picker and no second preference — someone who has said
   * "subtitles in Polish, else English" has already answered this question — and
   * it degrades to the interface language, which is what the old list gave that
   * person anyway. "Любые" stays last and always, or the panel would become
   * unable to find a subtitle in a language nobody thought to configure.
   */
  languages = $derived([
    ...(this.langPrefs.length ? this.langPrefs : [locale()]).map((code) => ({
      value: code,
      label: languageName(code),
    })),
    { value: '', label: t('subs.lang_any') },
  ]);

  /// A tenth of a frame is far tighter than any real pair (25 vs 23.976 is the
  /// case that matters) and loose enough to survive mpv reporting 23.976024.
  fpsOff(fps: number | null): boolean {
    return fps !== null && this.videoFps !== null && Math.abs(fps - this.videoFps) > 0.1;
  }
}

export const subs = new Subs();

/**
 * Bring the sign-in form fully into view, buttons included.
 *
 * Deliberately instant and deliberately done twice. Measured in a WKWebView
 * harness against this exact structure (a `max-height` box with `overflow-y:
 * auto`, the form inserted on click): every scroll API reaches the bottom —
 * `scrollIntoView`, `scrollTo`, `scrollTop`, smooth or not, with or without
 * waiting a frame — **except** when a `focus()` call is nearby, which trims a
 * *smooth* animation to 710 px of the 717 needed and does so even with
 * `preventScroll: true`. Instant lands exactly, so the animation is not worth
 * its one failure mode; and the second pass on the next frame covers the box
 * still settling as the form unfolds.
 */
export function revealSubsAuth() {
  subs.authEl?.scrollIntoView({ block: 'end' });
}

export async function openSubsAuth() {
  subs.authOpen = true;
  subs.authError = null;
  await tick();
  // Still `preventScroll`: focusing an input scrolls it into view by itself,
  // which lands the field near the top and leaves the buttons under the fold —
  // the half that mattered.
  subs.authUserEl?.focus({ preventScroll: true });
  revealSubsAuth();
  requestAnimationFrame(revealSubsAuth);
}

export async function loadSubsAccount(refresh: boolean) {
  try {
    subs.account = await invoke<SubsAccount>('subs_account', { refresh });
  } catch (e) {
    console.warn('account state failed:', e);
  }
}

export async function subsSignIn() {
  if (subs.authBusy || !subs.user.trim() || !subs.pass) return;
  subs.authBusy = true;
  subs.authError = null;
  try {
    subs.account = await invoke<SubsAccount>('subs_login', {
      username: subs.user.trim(),
      password: subs.pass,
      rememberPassword: subs.remember,
    });
    subs.authOpen = false;
    // Kept nowhere on this side: the password's only home is the keychain, and
    // only when it was asked for.
    subs.pass = '';
    // `/login` reports the account's allowance but not what is left of it today;
    // `/infos/user` does. Asked for straight away, because the line that says
    // "12 of 20 left" is also the proof that the token works.
    void loadSubsAccount(true);
  } catch (e) {
    subs.authError = t('subs.sign_in_failed', { reason: String(e) });
  } finally {
    subs.authBusy = false;
  }
}

export async function subsSignOut() {
  subs.authBusy = true;
  try {
    subs.account = await invoke<SubsAccount>('subs_logout');
    subs.pass = '';
  } catch (e) {
    console.warn('sign out failed:', e);
  } finally {
    subs.authBusy = false;
  }
}

/**
 * Take an external subtitle out of the session — and off the disk when it is one
 * we downloaded.
 *
 * The second half is not tidiness: a downloaded subtitle is named after the
 * video precisely so `sub-auto=fuzzy` finds it by itself next time, so detaching
 * without deleting means it is back at the next launch and the removal looks
 * broken. A file we did not create is only detached, because a subtitle the
 * viewer made or corrected is not ours to delete.
 */
export async function removeSubtitle(track: Track) {
  const path = track.path;
  if (!(await removeSubTrack(track))) return;
  if (path && isDownloadedSub(path)) {
    try {
      await invoke('subs_delete_file', { path });
      forgetDownloadedSub(path);
      showOsd(t('osd.sub_deleted'));
      return;
    } catch (e) {
      // The track is already gone from the session, so this is not a failure of
      // the action the viewer asked for — only of the cleanup.
      console.warn('deleting the subtitle file failed:', e);
    }
  }
  showOsd(t('osd.sub_removed'));
}

export async function openSubsDialog() {
  subs.error = null;
  subs.hits = null;
  subs.busyId = null;
  // **Asked of mpv, not of `settingsValues`** — that map is filled when the
  // settings dialog opens and is empty until then, so reading it here would make
  // the filter depend on whether the viewer had been into settings this session.
  // mpv always knows: it has merged mpv.conf over `initialOptions`, and every
  // change in that dialog is applied to it live.
  subs.langPrefs = parseLangList(await getProperty('slang', 'string').catch(() => null));
  // The first preferred subtitle language, which is where a search should start;
  // with none set, the language the UI is in. "Any" stays one click away either
  // way.
  subs.lang = subs.langPrefs[0] ?? locale();
  // Prefilled with the title the player is showing, which for a file mpv named
  // is already the right query — and is what the viewer would type. A release
  // name carries the show and the episode in one string, and both halves are
  // wanted: the numbers reach the right work (a title alone finds the 1973 film
  // rather than the 2024 series), and the title without them is a far better
  // query than the whole file name.
  subs.episode = player.filePath ? parseEpisode(player.filePath) : null;
  subs.query = subs.episode?.title || (player.hasFile ? player.displayTitle : '');
  subs.videoFps = player.hasFile
    ? await getProperty('container-fps', 'double').catch(() => null)
    : null;
  subs.open = true;
  subs.authOpen = false;
  subs.authError = null;
  // The live quota, which is worth knowing before spending one to find out.
  void loadSubsAccount(true);
  await tick();
  subs.inputEl?.focus();
  subs.inputEl?.select();
  // A local file can be searched by hash with nothing typed, so the panel opens
  // with an answer instead of an empty box.
  if (player.filePath && !player.filePath.includes('://')) void runSubsSearch();
}

export async function runSubsSearch() {
  if (subs.busy) return;
  subs.busy = true;
  subs.error = null;
  try {
    const result = await invoke<{
      items: SubHit[];
      match_kind: 'hash' | 'title';
      hash_tried: boolean;
      hash_blocked: boolean;
      total: number;
    }>('subs_search', {
      path: player.filePath ?? null,
      query: subs.query.trim() || null,
      languages: subs.lang,
      season: subs.episode?.season ?? null,
      episode: subs.episode?.episode ?? null,
    });
    subs.hits = result.items;
    subs.matchKind = result.match_kind;
    subs.hashTried = result.hash_tried;
    subs.hashBlocked = result.hash_blocked;
  } catch (e) {
    console.warn('subtitle search failed:', e);
    subs.hits = null;
    subs.error = String(e).includes('no API key') ? t('subs.no_key') : t('subs.failed');
  } finally {
    subs.busy = false;
  }
}

export async function downloadSub(hit: SubHit) {
  if (subs.busyId !== null) return;
  subs.busyId = hit.file_id;
  subs.error = null;
  try {
    const result = await invoke<{
      path: string;
      remaining: number | null;
      reset: string | null;
    }>('subs_download', {
      fileId: hit.file_id,
      videoPath: player.filePath ?? null,
      language: hit.language || 'sub',
    });
    subs.quota = { remaining: result.remaining, reset: result.reset };
    // Recorded as ours before it is attached: this is the only thing that later
    // tells it apart from a subtitle the viewer made, and it is what lets
    // removing it delete the file instead of leaving one `sub-auto` brings
    // straight back.
    rememberDownloadedSub(result.path);
    // Straight through the same path a dropped .srt takes, so it is selected and
    // the track list refreshes without a second mechanism.
    await attachTrack('sub', result.path);
    subs.open = false;
  } catch (e) {
    console.warn('subtitle download failed:', e);
    // A spent quota is a state with a way out, not a breakage: measured, the API
    // answers HTTP 406 with the time it renews. Anonymous callers get 5 a day
    // per IP, so this is the wall a regular viewer hits — and signing in is what
    // moves it, which is why the form is opened right here rather than left to
    // be discovered.
    const failure = e as { kind?: string; message?: string; reset?: string | null };
    if (failure?.kind === 'quota') {
      subs.error = failure.reset
        ? t('subs.quota_spent_reset', { time: failure.reset })
        : t('subs.quota_spent');
      if (!subs.account?.signed_in) void openSubsAuth();
    } else {
      subs.error = t('subs.download_failed_hint', {
        reason: failure?.message ?? String(e),
      });
    }
  } finally {
    subs.busyId = null;
  }
}

/**
 * An audio file, played as one: its cover, its tags, its lyrics, and an album
 * that plays on from track to track.
 *
 * A file is an audio file when it has a sound track and no picture to watch —
 * cover art is a video track to mpv, flagged `albumart`, and does not count
 * (`player.pictureTracks`). That is known only once the track list has been
 * read for *this* file (`player.tracksOf`), a beat after it opens; until then
 * the extension stands in, so that an album going from one track to the next
 * keeps its screen up and a film after a song does not flash the song's cover.
 *
 * The cover is the web view's (`audio-display=no` in session.ts): the picture
 * in the file, else one beside it that is plainly the cover (`cover_art` in
 * audio_meta.rs). mpv's own drawing would make it the video, with the window
 * fitted to its square at its own size.
 *
 * **An album plays on by itself.** The end of a file is ours (`keep-open=always`,
 * the end screen and its countdown) — right for an episode, wrong between two
 * songs, where the screen and the five seconds are a gap in the music. So an
 * audio file gets mpv's own `keep-open=yes` for itself
 * (`file-local-options`, gone with the file): mpv moves to the next entry the
 * moment this one ends, keeps the audio output open across (`gapless-audio`,
 * mpv's default `weak`, for tracks in one format) and has the next file opened
 * ahead of time (`prefetch-playlist`, local files only — a stream's next entry
 * is somebody's bandwidth). It stops on the last entry as before, and wraps
 * under repeat-all as `loop-playlist` always could. Not while casting or in a
 * room — both move the queue themselves — and not when the viewer has turned
 * auto-advance off.
 */

import { invoke } from '@tauri-apps/api/core';
import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { curtain, liftCurtain } from './curtain.svelte';
import { playback } from './playback.svelte';
import { lyricLine } from './music';
import { isAudioPath, isNetworkSource, player } from './player.svelte';
import { playlist } from './playlist.svelte';
import { wire } from './sync/wire.svelte';

export interface AudioTags {
  artist: string | null;
  album: string | null;
  year: string | null;
}

class AudioScreen {
  /// The open file is an audio file and the screen is up.
  on = $state(false);
  /// An object URL of the cover, or null for none (yet).
  cover = $state<string | null>(null);
  tags = $state<AudioTags>({ artist: null, album: null, year: null });
  /// The line of the lyrics being sung, when the file has any (an `.lrc`
  /// beside it is found by `sub-auto` like a subtitle) and they are on.
  lyric = $derived(this.on ? lyricLine(player.subText) : '');
}

export const audioScreen = new AudioScreen();

/// What the cover and the tags were last read for, so a track-list re-read
/// (tracks keep arriving after `file-loaded`) does not read them again.
let readFor: string | null = null;
let coverFor: string | null = null;

async function tag(key: string): Promise<string | null> {
  const v = await getProperty(`metadata/by-key/${key}`, 'string').catch(() => null);
  return v?.trim() || null;
}

async function readTags(path: string) {
  const [artist, albumArtist, album, date] = await Promise.all([
    tag('artist'),
    tag('album_artist'),
    tag('album'),
    tag('date'),
  ]);
  if (player.filePath !== path) return;
  audioScreen.tags = {
    artist: artist ?? albumArtist,
    album,
    year: date?.match(/\d{4}/)?.[0] ?? null,
  };
}

async function readCover(path: string) {
  // The cover of the previous track stays up until this one's is known: an
  // album's tracks nearly always share one, and a blank between them would be
  // a blink on every track change.
  const bytes = isNetworkSource(path)
    ? null
    : await invoke<ArrayBuffer>('cover_art', { path }).catch(() => null);
  if (player.filePath !== path) return;
  const old = audioScreen.cover;
  audioScreen.cover = bytes && bytes.byteLength > 0 ? URL.createObjectURL(new Blob([bytes])) : null;
  coverFor = path;
  if (old) URL.revokeObjectURL(old);
}

/// mpv's end-of-file mode and prefetch for this file — see the module comment.
function playOn(local: boolean, on: boolean) {
  void command('set', ['file-local-options/keep-open', on ? 'yes' : 'always']).catch(() => {});
  void command('set', ['file-local-options/prefetch-playlist', on && local ? 'yes' : 'no']).catch(
    () => {},
  );
}

/// The standing effects. Call during a component's initialization — see the
/// `$effect` rule in docs/rules/frontend.md.
export function initAudioScreen() {
  // Up or down. Settled by the tracks once they are read for this file;
  // before that, by the extension.
  $effect(() => {
    const path = player.filePath;
    if (!path) return;
    if (player.tracksOf !== path) {
      if (!isAudioPath(path)) audioScreen.on = false;
      return;
    }
    audioScreen.on = player.audioTracks.length > 0 && player.pictureTracks === 0;
  });

  // What it shows. Not before the tracks are read for this file: the screen
  // stays up from one song to the next, and at the moment `path` moves mpv
  // has not opened the new file yet — its tags read back as unavailable.
  $effect(() => {
    const path = player.filePath;
    if (!audioScreen.on || !path || player.tracksOf !== path || readFor === path) return;
    readFor = path;
    void readTags(path);
    void readCover(path);
    // There is no picture coming for the curtain to wait for, and no window
    // shape to morph to: the screen is what is there.
    if (curtain.on) liftCurtain();
  });

  // Dropped with the screen, so the next audio file never opens on the last
  // one's cover from minutes ago.
  $effect(() => {
    if (audioScreen.on) return;
    readFor = null;
    if (coverFor !== null && audioScreen.cover) URL.revokeObjectURL(audioScreen.cover);
    coverFor = null;
    audioScreen.cover = null;
    audioScreen.tags = { artist: null, album: null, year: null };
  });

  // The album plays on. Re-decided when anything it depends on changes
  // mid-file: a room joined, a cast started, auto-advance switched off.
  $effect(() => {
    const path = player.filePath;
    if (!audioScreen.on || !path || player.tracksOf !== path) return;
    playOn(!isNetworkSource(path), playlist.autoAdvance && !playback.session && !wire.on);
  });
}

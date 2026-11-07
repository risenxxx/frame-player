/**
 * Audio dropped onto a playing film: a dub to attach, or something to play.
 *
 * The extension cannot say — a `.flac` is as likely a lossless dub as an album
 * track — so the length does. A dub runs as long as the picture it was cut
 * for, give or take a trimmed logo or credits; a song is minutes against a
 * film's hours. A file named after the video is a dub whatever its length,
 * which is what mpv's own `audio-file-auto` goes by too.
 */

/// How far a dub may run from the film: 5 %, and never less than ten seconds,
/// so a short clip's dub with a second of silence on the end still counts.
export function dubTolerance(videoSec: number): number {
  return Math.max(videoSec * 0.05, 10);
}

function stem(path: string): string {
  const name = path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
  const dot = name.lastIndexOf('.');
  return (dot > 0 ? name.slice(0, dot) : name).toLowerCase();
}

export function looksLikeDub(
  audioPath: string,
  audioSec: number | null,
  videoPath: string | null,
  videoSec: number,
): boolean {
  if (videoPath) {
    const video = stem(videoPath);
    if (video && stem(audioPath).startsWith(video)) return true;
  }
  // Nothing to compare: the film's length is not known yet (a stream still
  // opening) or the file's cannot be read. Attaching is what a drop onto a film
  // always did, and the track can be switched back.
  if (!(videoSec > 0) || audioSec == null || !(audioSec > 0)) return true;
  return Math.abs(audioSec - videoSec) <= dubTolerance(videoSec);
}

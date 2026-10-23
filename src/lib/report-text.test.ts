import { describe, expect, it } from 'vitest';

import { assemble, describeValue, settingsLines } from './report-text';

describe('settings in a report', () => {
  it('prints choices and hides anything shaped like an address, a path or a credential', () => {
    const lines = settingsLines([
      [
        'torrent',
        JSON.stringify({
          seeding: false,
          encryption: 'prefer',
          route: 'auto',
          proxy: 'socks5://anna:hunter2@10.0.0.1:1080',
          dir: '/Volumes/Films',
          port: 6881,
        }),
      ],
      ['indexerApiKey', 'abc123'],
      ['relay', 'wss://relay.example.org'],
      ['locale', 'ru'],
      ['missing', null],
    ]);
    const text = lines.join('\n');
    expect(text).toContain('seeding=false');
    expect(text).toContain('encryption=prefer');
    expect(text).toContain('port=6881');
    for (const leak of ['hunter2', 'anna', '10.0.0.1', 'Films', 'abc123', 'relay.example']) {
      expect(text, leak).not.toContain(leak);
    }
    expect(text).toContain('locale: ru');
    expect(lines).toHaveLength(4);
  });

  it('does not print a free-form string even under an innocent key', () => {
    expect(describeValue('label', 'Anna’s room')).toBe('set');
    expect(describeValue('mode', 'fullscreen')).toBe('fullscreen');
  });

  it('summarises lists and nested records instead of printing them', () => {
    expect(describeValue('roots', ['/Users/anna/Private'])).toBe('[1]');
    expect(describeValue('picture', { a: { b: 1 } })).toBe('{a={1}}');
  });
});

describe('the assembled report', () => {
  it('has every section, and says where the mpv log went when it is not ours', () => {
    const text = assemble({
      generatedAt: new Date(Date.UTC(2026, 9, 23, 12, 0, 0)),
      summary: [['Version', '1.34.0']],
      playback: [['Source', 'nothing open']],
      casting: null,
      settings: [],
      frontend: { lines: ['12:00:00.000 [warn] x'], dropped: 0 },
      native: { lines: [], dropped: 3, capture: 'captured' },
      mpv: null,
      mpvNote: '(mpv is not writing a log)',
    });
    expect(text).toContain('Generated 2026-10-23 12:00:00 UTC');
    expect(text).toContain('## Interface log (1 lines)');
    expect(text).toContain('## Native log (0 lines, 3 older dropped; stderr captured)');
    expect(text).toContain('(mpv is not writing a log)');
    expect(text).not.toContain('## Casting');
  });
});

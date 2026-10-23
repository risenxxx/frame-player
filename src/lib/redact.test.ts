import { describe, expect, it } from 'vitest';

import { redact, type Literal } from './redact';

const r = (text: string, literals: Literal[] = []) => redact(text, literals).text;

/// Every sensitive fragment of a sample must be gone from its redaction —
/// asserted as absence rather than as an exact string wherever the exact
/// output is not the point, so a better placeholder later does not fail a test
/// whose job is to catch a leak.
function gone(out: string, ...fragments: string[]) {
  for (const f of fragments) expect(out, `"${f}" leaked`).not.toContain(f);
}

describe('paths', () => {
  it('hides a macOS path and keeps the container', () => {
    expect(r('[thumbs] failed for /Users/anna/Movies/Secret.Film.2019.1080p.mkv: EOF')).toBe(
      '[thumbs] failed for <path>.mkv: EOF',
    );
  });

  it('follows spaces and parentheses to the extension', () => {
    const out = r('Playing: /Users/anna/My Movies/Secret Film (2019)/Secret Film.mkv at 12');
    expect(out).toBe('Playing: <path>.mkv at 12');
  });

  it('hides a Windows path, also JSON-escaped and with forward slashes', () => {
    gone(r('open C:\\Users\\Ivan\\Видео\\фильм.mkv'), 'Ivan', 'фильм', 'Видео');
    gone(r('{"path":"C:\\\\Users\\\\Ivan\\\\Videos\\\\film.mkv"}'), 'Ivan', 'film');
    gone(r('url="D:/Сериалы/Тайное/e01.mkv"'), 'Сериалы', 'Тайное');
    expect(r('open C:\\Users\\Ivan\\film.mkv')).toBe('open <path>.mkv');
  });

  it('hides a UNC share and a home-relative path', () => {
    gone(r('reading \\\\nas-anna\\films\\a.mkv'), 'nas-anna', 'films');
    gone(r('config at ~/Library/Application Support/x.conf'), 'Application Support');
  });

  it('keeps system paths, which name no one', () => {
    const line = 'dyld: loaded /System/Library/Frameworks/AVKit.framework/AVKit';
    expect(r(line)).toBe(line);
    expect(r('C:\\Windows\\System32\\vulkan-1.dll missing')).toBe('C:\\Windows\\System32\\vulkan-1.dll missing');
  });

  it('leaves MIME types, module paths and fractions alone', () => {
    const line = 'video/x-matroska from src/lib.rs:12 at 1/2 speed';
    expect(r(line)).toBe(line);
  });

  it('does not stop at a dotted folder name', () => {
    gone(r('in /Users/anna/v1.2 final/a.mkv now'), 'final', 'anna');
  });
});

describe('URLs', () => {
  it('keeps a loopback server and its route, not the hash or the name', () => {
    const hash = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
    expect(r(`http://127.0.0.1:51234/t/${hash}/0/Sintel.2010.mkv`)).toBe(
      'http://127.0.0.1:51234/t/<path>.mkv',
    );
  });

  it('aliases the LAN server and drops the cast token', () => {
    const out = r('[airplay] open http://192.168.1.34:52817/c/9f3a8e7d6c5b4a39281706f5e4d3c2b1/The%20Film%20(2020).mkv at 0.0');
    expect(out).toBe('[airplay] open http://lan-1:52817/c/<path>.mkv at 0.0');
  });

  it('hides a link, its query and its host', () => {
    const out = r('loading https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=AbCdEf123');
    gone(out, 'youtube', 'dQw4w9WgXcQ', 'AbCdEf123');
    expect(out).toBe('loading https://<host>/<path>?<query>');
  });

  it('keeps the hosts the player itself talks to', () => {
    expect(r('GET https://api.opensubtitles.com/api/v1/subtitles?query=secret+film failed')).toBe(
      'GET https://api.opensubtitles.com/<path>?<query> failed',
    );
    expect(r('see https://github.com/risenxxx/frame-player/issues/new.')).toBe(
      'see https://github.com/risenxxx/frame-player/issues/new.',
    );
  });

  it('drops credentials and a private indexer', () => {
    const out = r('proxy socks5://anna:hunter2@10.8.0.1:1080 indexer http://jackett.home.arpa:9117/api/v2.0/indexers/all/results/torznab/api?apikey=0123456789abcdef');
    gone(out, 'anna', 'hunter2', 'jackett', '0123456789abcdef', '10.8.0.1');
  });

  it('hides a magnet whole', () => {
    expect(r('add magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=Secret+Film&tr=udp%3A%2F%2Ftracker.example.org%3A1337')).toBe(
      'add <magnet>',
    );
  });

  it('stops at the escaped brackets DIDL-Lite travels in', () => {
    const out = r('&lt;res protocolInfo=&quot;http-get:*:video/mp4:*&quot;&gt;http://192.168.1.34:50000/c/tok/Film.mp4&lt;/res&gt;');
    expect(out).toContain('http://lan-1:50000/c/<path>.mp4&lt;/res&gt;');
  });
});

describe('addresses', () => {
  it('gives every LAN address one alias for the whole report', () => {
    const out = r('[dlna] 192.168.1.20:1900 -> http://192.168.1.20:1468/DMR/\n[cast] http 192.168.1.20 GET from 192.168.1.34');
    expect(out).toBe('[dlna] lan-1:1900 -> http://lan-1:1468/<path>\n[cast] http lan-1 GET from lan-2');
  });

  it('keeps loopback, multicast and broadcast', () => {
    const line = 'M-SEARCH to 239.255.255.250:1900, mDNS 224.0.0.251, bound 0.0.0.0 and 127.0.0.1, ::1';
    expect(r(line)).toBe(line);
  });

  it('separates public from private', () => {
    expect(r('relay 203.0.113.7 via 10.0.0.2 and 100.64.1.1')).toBe('relay ip-1 via lan-1 and lan-2');
  });

  it('aliases IPv6, link-local with its zone', () => {
    expect(r('from fe80::1c2b:3d4e:5f60:7182%en0 and 2001:db8::42')).toBe('from lan-1%en0 and ip-1');
  });

  it('leaves clocks, versions and Rust paths alone', () => {
    const line = '12:34:56.789 [  12.345] std::io::Error mpv 0.41.0 libavcodec 61.19.100 build 26100.4061';
    expect(r(line)).toBe(line);
  });

  it('hides MAC addresses, UUIDs and mDNS hostnames', () => {
    const out = r('deviceid=AA:BB:CC:DD:EE:FF udn uuid:4fd8c2a1-1b3c-4d5e-8f90-a1b2c3d4e5f6 host Annas-MacBook-Pro.local.');
    gone(out, 'AA:BB', '4fd8c2a1', 'Annas');
    expect(out).toContain('uuid:uuid-1');
    expect(out).toContain('<host>.local');
  });

  it('keeps a service type that ends in .local', () => {
    const line = 'browsing _googlecast._tcp.local. and _airplay._tcp.local.';
    expect(r(line)).toBe(line);
  });
});

describe('secrets', () => {
  it('hides API keys, tokens and passwords in any spelling', () => {
    const out = r(
      [
        'Api-Key: 9a8b7c6d5e4f3a2b1c0d',
        '{"token":"eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"}',
        'password=hunter2&username=anna',
        'Authorization: Bearer s3cr3t-T0ken-value',
      ].join('\n'),
    );
    gone(out, '9a8b7c6d5e4f3a2b1c0d', 'eyJ', 'hunter2', 'anna', 's3cr3t');
  });

  it('hides a long random token that has no key in front of it', () => {
    gone(r('ticket X9vQ2mL7pR4sT8wY1zA6bC3dE5fG0hJ'), 'X9vQ2mL7pR4sT8wY1zA6bC3dE5fG0hJ');
  });

  it('keeps counts and flags that follow a secret-sounding key', () => {
    expect(r('sessions: 2, key: true')).toBe('sessions: 2, key: true');
  });
});

describe('protocol samples', () => {
  it('Cast: a LOAD on the wire', () => {
    const line =
      '[cast] -> urn:x-cast:com.google.cast.media {"type":"LOAD","requestId":3,"sessionId":"8c2b7d4e-1f2a-4b3c-9d8e-7f6a5b4c3d2e","media":{"contentId":"http://192.168.1.34:52817/c/9f3a8e7d6c5b4a39281706f5e4d3c2b1/Secret%20Film.mkv","streamType":"BUFFERED","contentType":"video/x-matroska","metadata":{"type":0,"metadataType":0,"title":"Secret Film"}},"autoplay":true,"currentTime":12.5}';
    const out = r(line);
    gone(out, '8c2b7d4e', '9f3a8e7d', 'Secret', '192.168');
    expect(out).toContain('"contentType":"video/x-matroska"');
    expect(out).toContain('"currentTime":12.5');
  });

  it('Cast: a receiver status and the eureka name', () => {
    const out = r(
      '[cast] <- urn:x-cast:com.google.cast.receiver {"requestId":2,"status":{"applications":[{"appId":"CC1AD845","displayName":"Default Media Receiver","sessionId":"0b3f9c1e-2d4a-4e5f-8a9b-1c2d3e4f5a6b","statusText":"Ready To Cast","transportId":"0b3f9c1e-2d4a-4e5f-8a9b-1c2d3e4f5a6b"}]},"type":"RECEIVER_STATUS"}\n' +
        '{"name":"Anna\'s Living Room","ssdp_udn":"4fd8c2a1-1b3c-4d5e-8f90-a1b2c3d4e5f6","mac_address":"AA:BB:CC:DD:EE:FF","ssid":"Anna Home 5G"}',
    );
    gone(out, '0b3f9c1e', 'Living Room', '4fd8c2a1', 'AA:BB', 'Anna Home');
    expect(out).toContain('"appId":"CC1AD845"');
    expect(out).toContain('RECEIVER_STATUS');
  });

  it('Cast: our own discovery line, through the literal', () => {
    const out = r('[cast] unicast: Living Room TV at 192.168.1.20', [{ kind: 'device', value: 'Living Room TV' }]);
    expect(out).toBe('[cast] unicast: <device> at lan-1');
  });

  it('DLNA: a description', () => {
    const xml =
      '<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>[TV] Anna&apos;s Bedroom</friendlyName><manufacturer>LG Electronics</manufacturer><modelName>OLED55C1</modelName><serialNumber>105MXSJ2K123</serialNumber><UDN>uuid:4fd8c2a1-1b3c-4d5e-8f90-a1b2c3d4e5f6</UDN></device></root>';
    const out = r(xml);
    gone(out, 'Bedroom', '105MXSJ2K123', '4fd8c2a1');
    expect(out).toContain('<modelName>OLED55C1</modelName>');
    expect(out).toContain('<manufacturer>LG Electronics</manufacturer>');
  });

  it('DLNA: SetAVTransportURI with its DIDL-Lite metadata', () => {
    const soap =
      '[dlna] SetAVTransportURI <CurrentURI>http://192.168.1.34:50123/c/0123456789abcdef0123456789abcdef/Secret.Film.mkv</CurrentURI><CurrentURIMetaData>&lt;DIDL-Lite&gt;&lt;item&gt;&lt;dc:title&gt;Secret Film&lt;/dc:title&gt;&lt;res protocolInfo=&quot;http-get:*:video/x-matroska:*&quot;&gt;http://192.168.1.34:50123/c/0123456789abcdef0123456789abcdef/Secret.Film.mkv&lt;/res&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</CurrentURIMetaData>';
    const out = r(soap);
    gone(out, 'Secret', '0123456789abcdef', '192.168');
    expect(out).toContain('video/x-matroska');
  });

  it('DLNA: GetTransportInfo keeps the states', () => {
    const line =
      '[dlna] GetTransportInfo -> <?xml version="1.0"?><s:Envelope><s:Body><u:GetTransportInfoResponse><CurrentTransportState>PLAYING</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus></u:GetTransportInfoResponse></s:Body></s:Envelope>';
    expect(r(line)).toBe(line);
  });

  it('AirPlay: the state line and a TXT record', () => {
    expect(r('[airplay] buffering - peer=192.168.1.20 waiting=AVPlayerWaitingToMinimizeStallsReason t=0.0 dur=0.0')).toBe(
      '[airplay] buffering - peer=lan-1 waiting=AVPlayerWaitingToMinimizeStallsReason t=0.0 dur=0.0',
    );
    const txt = r(
      'deviceid=AA:BB:CC:DD:EE:FF, features=0x5A7FFEE6,0x0, model=AppleTV6,2, pk=3fa9c0d18e7b6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b2a19, pi=2e1d0c9b-8a7f-4e5d-9c3b-2a1908f7e6d5, name=Anna\'s Bedroom',
    );
    gone(txt, 'AA:BB', '3fa9c0d1', '2e1d0c9b', 'Bedroom');
    expect(txt).toContain('model=AppleTV6,2');
  });

  it('AirPlay: a plist', () => {
    const out = r('<dict><key>name</key><string>Anna’s Bedroom</string><key>model</key><string>AppleTV11,1</string></dict>');
    gone(out, 'Bedroom');
    expect(out).toContain('<string>AppleTV11,1</string>');
  });

  it("mpv: a file's tags and the command that opened it", () => {
    const log = [
      '[   0.215][i][cplayer] Playing: /Users/anna/Movies/Secret Film (2019).mkv',
      '[   0.402][i][cplayer] File tags:',
      '[   0.402][i][cplayer]  title: Secret Film',
      '[   0.402][i][cplayer]  comment: ripped by anna',
      '[   0.950][d][cplayer] Run command: loadfile, flags=64, args=[url="C:\\Users\\Ivan\\Видео\\фильм.mkv", flags="replace"]',
      '[   1.010][v][vd] Using hardware decoding (videotoolbox).',
    ].join('\n');
    const out = r(log);
    gone(out, 'Secret', 'anna', 'Ivan', 'фильм');
    expect(out).toContain('Using hardware decoding (videotoolbox).');
  });
});

describe('what a real report showed', () => {
  it('keeps Vulkan extension names and versions', () => {
    const line = '    VK_KHR_get_physical_device_properties2\n    Conformance version: 1.3.8.0';
    expect(r(line)).toBe(line);
  });

  it('follows a folder with spaces to its trailing separator', () => {
    const out = r('[find_files] Loading external files in /tmp/x/Secret Film (2019)/');
    gone(out, 'Secret', 'Film', '2019');
  });

  it("keeps the GPU's and the driver's names, which follow another word", () => {
    const line = '    Device Name: Apple M3 Max\n    Driver name: MoltenVK';
    expect(r(line)).toBe(line);
    expect(r('[display-tags]  Title: Secret Film')).toBe('[display-tags]  Title: <name>');
  });

  it("hides the name of the audio output, which may be somebody's headphones", () => {
    expect(r("[ao/coreaudio] selected audio output device: Anna's AirPods Pro (77)")).toBe(
      '[ao/coreaudio] selected audio output device: <device> (77)',
    );
  });
});

describe('literals', () => {
  it('replaces the file wherever it went, its name and its folder too', () => {
    const out = r('queued Episode.05.mkv from Breaking Season 1; [playlist] Breaking Season 1 has 12 videos', [
      { kind: 'file', value: '/Volumes/Media/Breaking Season 1/Episode.05.mkv' },
    ]);
    gone(out, 'Episode.05', 'Breaking Season');
    expect(out).toContain('<file>.mkv');
  });

  it('matches a name as a word, not inside another one', () => {
    expect(r('user anna opened annals', [{ kind: 'user', value: 'anna' }])).toBe('user <user> opened annals');
  });

  it('takes the user from the home directory', () => {
    expect(r('owner anna', [{ kind: 'home', value: '/Users/anna' }])).toBe('owner <user>');
  });

  it('ignores names too short to replace safely', () => {
    expect(r('TV on', [{ kind: 'device', value: 'TV' }])).toBe('TV on');
  });

  it('counts what it replaced', () => {
    const { counts } = redact('a 192.168.1.2 b 192.168.1.3 /Users/a/b.mkv', []);
    expect(counts).toMatchObject({ lan: 2, path: 1 });
  });
});

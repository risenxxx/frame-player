# The zh-CN glossary

The Chinese interface (`zh` in `src/lib/i18n.svelte.ts`) is translated from the
**English** strings, with the Russian as a second reference for meaning, into
Simplified Chinese as mainland desktop software writes it (Windows 11 and macOS
zh-CN conventions). This file is what keeps a new string in step with the 900
already there: read it before adding a key, and add a row when a string needs a
term it does not have.

How the first translation was made, and what to repeat for a large batch: a
group of keys translated against this file, the mechanical checks
(`src/lib/i18n.test.ts` covers placeholders and stray Cyrillic; a hint that
quotes a label must quote that label's zh exactly), then an independent review
by a model trained mostly on Chinese, adjudicated against this file — its
suggestions were right about idiom far more often than about terms. The terms
marked *verified* were checked against qBittorrent's own zh_CN translation,
which settled two disagreements the reviews could not. After the release a
native speaker read every string; they kept all the terms below and their
wording fixes are in, with the rows they added marked *native review*.

The macOS menu bar (`src-tauri/src/macos_menu.rs`) is the one exception to
these rules: it follows Apple's zh-CN menu wording (全屏幕, 缩放, 打开位置…).

## Style rules

| Rule | Decision |
|---|---|
| Pronoun | **你**, and avoid a pronoun where the sentence works without one (Windows 11 and current macOS use 你) |
| Punctuation | Full-width in Chinese text: ，。：；？！（） |
| Quoting another UI label | “…” (mainland quotes), and the quoted text must match that label's own translation **exactly** |
| Final period | Mirrors the English: a label without a period gets none; a sentence hint ending in "." ends in 。 |
| Menu items that open a dialog | Keep the single “…” the English has (`打开文件…`), as Windows/macOS zh-CN menus do |
| Em dash "—" | Usually becomes ，/：/（）. Use “——” only where a real break in thought remains |
| Spaces | Half-width space between Han characters and Latin words/digits/placeholders (`Windows HDR 当前已关闭`, `{name} 加入了房间`); no space next to full-width punctuation |
| Placeholders | `{name}` etc. copied verbatim, never translated or removed |
| Units | `1 秒`, `{value}×`, `fps` stays `fps` (common in zh video software); `MB`, `GB` stay |
| Lists | A list built at runtime goes through `joinList`, which writes 、 here; never `.join(', ')` on anything a viewer reads |
| Hover / over a control | 悬停在…上 / 位于…上; 上方 only for literally “above” |
| Pills | Sibling pills are translated together as a parallel, short set; On/Off → 开启/关闭; a hint quoting a pill is re-checked whenever the pill changes |
| Counts | Every count gets a measure word (条/个/位/集) |
| Qualifiers | Keep every qualifier the English has: measured, by default, never, only, completely, legal |
| Re-read | After translating a hint, re-read it as a Chinese user; rewrite anything that only makes sense back-translated (不覆盖, 绝不会更差) |
| Ellipsis | Single “…” in menu items **and in progress/status lines** (正在保存当前帧…, 正在连接…), as Windows and macOS do; …… only inside a running sentence that trails off |
| Digits and symbols | Always half-width (0–9, %, ×, +, −, /, →); `%` and `×` bind to the number with no space (50%, 1.5×) |
| Frame rate | 帧率 as a label or table row; `fps` only inline after a value (`{sub} fps`) |
| Additions | Adding a qualifier the English lacks is as wrong as dropping one (en is the source; where ru says more, follow en and flag the difference) |
| Dialog buttons | 确定 / 取消 / 应用 / 保存 / 关闭; 暂不 for “not now”; 不再显示. The dialogs are HTML and shared by both platforms, so no 好 / 存储 split; only the native macOS menu (Rust) follows Apple's wording |
| Length | Chinese is denser; don't pad to match English length, but don't drop information either |

## Never translated

Frame Player, mpv, mpv.conf, FFmpeg, yt-dlp, OpenSubtitles, BitTorrent, DHT, MSE, Torznab, Jackett, Prowlarr, TMDB, YouTube, Google Cast, Chromecast, DLNA, UPnP, SSDP, AVTransport, TLS, SOCKS5, VPN, HDR, SDR, EDR, RSS, Windows, macOS, codec names, key names.

## Terms

### Playback and picture
| English | zh-CN | Note |
|---|---|---|
| player | 播放器 | |
| mini player | 迷你播放器 | |
| controls / control bar | 控制栏 | |
| fullscreen / in fullscreen | 全屏 / 全屏时 | |
| window | 窗口 | |
| cursor | 指针 (鼠标指针 in a setting label where 指针 alone is ambiguous) | OS term, not 光标 |
| seekbar | 进度条 | |
| seek / seeking | 跳转 | “seek forward 5 s” in hotkeys → 快进 5 秒 |
| exact (seek) | 精确 | |
| playlist | 播放列表 | |
| queue | 播放队列 | |
| up next (end screen) | 接下来播放 | YouTube zh-CN; 即将播放 only for a list of what is queued |
| file | 文件 | |
| frame / frame step / save frame | 帧 / 逐帧 / 保存当前帧 | |
| chapter | 章节 | |
| intro / credits / recap / preview | 片头 / 片尾 / 前情提要 / 预告 | |
| speed | 倍速 for the control, its tooltip and the OSD (`倍速：{value}×`); 速度 in running prose | every mainland video app labels the picker 倍速 |
| volume / mute | 音量 / 静音 | |
| repeat / loop | 循环 | `Repeat: all` → 循环：全部 |
| A–B loop / segment | A-B 循环 / 片段 | |
| aspect ratio | 画面比例 | |
| crop / black bars | 裁剪 / 黑边 | |
| zoom | 缩放 | |
| brightness / contrast / saturation / gamma | 亮度 / 对比度 / 饱和度 / 伽马 | |
| tone mapping | 色调映射 | |
| hardware / software decoding | 硬件解码 / 软件解码 | short pill label: 硬件 / 软件 |
| media information | 媒体信息 | |
| A/V sync (the desync value in media info) | 音画偏差 | |
| dropped frames | 丢帧 | |
| stalls (a process stopping) | 卡顿 | *native review*; not 停顿 |
| buffered | 已缓冲 | |

### Tracks and subtitles
| English | zh-CN | Note |
|---|---|---|
| audio track | 音轨 | |
| subtitles / subtitle track | 字幕 / 字幕轨 | |
| dub | 配音 | |
| delay / later / earlier | 延迟 / 延后 / 提前 | |
| hearing impaired (badge) | 听障字幕 | a bare adjective has no noun |
| search by title | 按片名搜索 | |
| sign in / sign out | 登录 / 退出登录 | |

### Torrents
| English | zh-CN | Note |
|---|---|---|
| torrent (the thing being downloaded) | 种子 | the everyday mainland word (迅雷, “BT 种子”). qBittorrent zh-CN keeps “Torrent” in Latin; we don't, the audience is viewers rather than BT power users |
| .torrent file | 种子文件 | |
| magnet link | 磁力链接 | |
| peer | 用户 | verified in qBittorrent zh_CN (Peers → 用户; 节点 is what DHT nodes are, so 节点 would collide). **Only** for peers: people in a room are 成员/人, the viewer is 你 |
| seeder / leecher | 做种者 / 下载者 | |
| announce (to trackers/DHT) | 汇报 | verified in qBittorrent zh_CN (强制重新汇报); never 公布自己 / 通告 |
| handshake | 握手 | |
| swarm | — | avoid the word; rephrase with 用户 |
| uploading / seeding | 上传 / 做种 | setting name uses 做种 |
| piece | 文件块 | qBittorrent zh_CN |
| tracker (protocol) | Tracker | kept in Latin, as in qBittorrent |
| tracker (the website) | 种子站; in running hints prefer 发布该种子的网站. Not “tracker 网站” | 资源站 reads as a pirate streaming site in mainland usage |
| release (a torrent) | 资源 | an app release / update is 版本 / 更新 |
| re-upload / newer upload | 新版种子 | |
| RSS feed | RSS 订阅 | |
| episode / season | 集 / 季 (`第 {n} 季`) | **new episodes = 新集 (新集更新, 新集上线), never 新剧集** — 剧集 is a whole series. A model translating without this file reaches for 新剧集 unprompted, so this one needs watching |
| proxy | 代理 | |
| port forwarding | 端口转发 | |
| encryption | 加密 | modes are the qBittorrent set: 禁用加密 / 允许加密 (= “when possible”) / 强制加密 |
| route | 出口路由 | |
| ISP | 运营商 | |
| download / cache | 下载 / 缓存 | |

### Casting
| English | zh-CN | Note |
|---|---|---|
| cast / casting | 投屏 | tooltips and buttons too: “Watch on TV” → 投屏到电视, not 在电视上观看 |
| TV | 电视 | |
| device | 设备 | |
| local network | 本地网络 | |
| private network | 专用网络 | Windows zh-CN wording |
| firewall | 防火墙 | |
| transport (Cast vs DLNA) | 传输方式 | |
| prepared copy (repacked for the TV) | 兼容副本 | *native review*; the cache of them is 兼容副本缓存, never 准备好的… |

### Watching together
| English | zh-CN | Note |
|---|---|---|
| watch together | 一起看 | |
| room / room server | 房间 / 房间服务器 | |
| member | 成员 | |
| join / leave | 加入 / 离开 | |
| shared | 共享 | |

### Catalog
| English | zh-CN | Note |
|---|---|---|
| catalog | 片库 | not 目录 (= folder/directory) |
| metadata service | 元数据服务 | |
| release search server | 资源搜索服务器 | |
| series / movie | 剧集 / 电影 | |

### Platform wording (separate keys per platform)
| English | zh-CN | Note |
|---|---|---|
| Explorer | 文件资源管理器 | |
| Finder | 访达 | official macOS zh-CN name |
| Trash (macOS) / Recycle Bin (Windows) | 废纸篓 / 回收站 | |
| Displays (macOS settings pane) | 显示器 | |
| System Settings | 系统设置 | |

### Added after the review
| English | zh-CN | Note |
|---|---|---|
| play while downloading (a torrent) | 边下边播 | the product's headline feature; not 流式传输 / 串流 |
| sequential download | 按顺序下载 | qBittorrent zh_CN |
| share ratio / seeds | 分享率 / 做种数 | qBittorrent zh_CN |
| verify existing data | 校验 | qBittorrent zh_CN (重新校验) |
| seek backward | 快退 | twin of 快进 |
| screenshot (with subtitles) vs save frame | 截图 vs 保存当前帧 | keep the two apart |
| Dolby Vision / HDR10 | 杜比视界 / HDR10 | |
| peak brightness / brightness headroom / nits | 峰值亮度 / 亮度余量 / 尼特 | |
| start screen | 开始界面 | not 启动页 (reads as a splash screen); 返回开始界面 |
| folder | 文件夹 | never 目录 (taken by nothing, but it collides with 片库's reasoning) |
| genre | 类型 | TMDB zh-CN |
| quality (of a stream) / traffic | 画质 / 流量 | |
| direct link / network stream | 直链 / 网络流 | 直链, not 直连 (= direct connection) |
| portable copy | 便携版 | not 绿色版 |
| info hash (when shown) / identity in prose | 哈希 / 标识 | |
| room host / invite link | 房主 / 邀请链接 | the room does have a host (`sync.host_badge`) |
| Ethernet / Wi-Fi | 以太网 / Wi-Fi | |
| snap (mini player to edges) | 贴边吸附 | not Windows' own Snap feature, so not 贴靠 |
| autoplay next | 自动连播 | |
| forced subtitles | 强制字幕 | |
| upscaling (the picture panel's neural-network scaler) / shader | 超分辨率 / 着色器 | not yet read by the native reviewer; 超分辨率 is the term Chinese players use for the same switch |
| off (as a mode pill) / animation | 关闭 / 动画 | the Pills rule; 动画, not 动漫, since the mode is for drawn animation of any origin |

### Rejected suggestions (so they are not re-litigated)
| Suggestion | Why not |
|---|---|
| Finder kept in Latin | macOS zh-CN calls it 访达 |
| 光标 for the cursor | Windows and macOS settings both say 指针 / 鼠标指针 |
| 节点 for peers, 通告 for announce | contradicted by qBittorrent zh_CN itself |
| ISP kept in Latin | 运营商 is what a viewer says |
| “——” as the default for “—” | reads as translationese in UI; the rule above stands |
| 软件解码 for the decoding pill | the row is already labelled 解码, siblings are 硬件 / 仅 VideoToolbox |
| 新剧集 for new episodes | see the episode row |
| separate 好 / 存储 buttons on macOS | our dialogs are HTML and shared |

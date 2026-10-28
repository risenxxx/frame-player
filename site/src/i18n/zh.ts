/*
  The Simplified Chinese site. Typed against en.ts, which defines every key;
  written against docs/zh-glossary.md and the player's own `zh` dictionary
  (src/lib/i18n/zh.ts), so the site calls a thing what the program calls it
  — 种子, 用户 for its peers, 投屏, 一起看, 片库, 边下边播.

  The glossary's style rules hold here too: full-width punctuation, a
  half-width space between Han characters and Latin words or digits, 你 where a
  pronoun is needed at all, every count with its measure word.
*/
import type { Translation } from './index.ts'

export const zh: Translation = {
  meta: {
    title: 'Frame Player：适用于 Windows 和 macOS 的视频播放器',
    description:
      '基于 mpv 的免费开源视频播放器，支持 Windows 和 macOS：种子边下边播，原画质投屏到电视，任何内容都能从上次的位置继续。',
    imageAlt: 'Frame Player：文件、链接、磁力链接，一个播放器全部搞定。',
  },

  nav: {
    toTop: 'Frame Player，回到顶部',
    home: 'Frame Player，返回首页',
    sections: '页面导航',
    open: '打开任何内容',
    torrents: '种子',
    tv: '投屏',
    together: '一起看',
    everything: '更多功能',
    download: '下载',
  },

  theme: {
    legend: '主题',
    systemTitle: '跟随系统',
    system: '跟随系统',
    light: '浅色',
    dark: '深色',
  },

  lang: {
    legend: '语言',
    hint: '此页面也有简体中文版。',
    open: '阅读中文版',
    dismiss: '关闭',
  },

  hero: {
    title: ['文件、链接、磁力链接，', '一个播放器全部搞定。'],
    lede:
      'Frame Player 是一款基于 mpv 的免费开源视频播放器，适用于 Windows 和 macOS。' +
      '支持种子边下边播，无需转码即可投屏到 Chromecast 和 DLNA 电视，还能和朋友一起看。',
    shot:
      'Frame Player 正在播放种子中的 4K 文件：进度条上方是后面某个场景的悬停预览，进度条上有章节标记，画面上有“跳过片头”按钮。',
  },

  download: {
    windows: '下载 Windows 版',
    macos: '下载 macOS 版',
    windowsArm: 'Windows ARM 版',
    version: '版本 {version}',
    platforms: 'Windows 10/11{sep}Apple Silicon',
    license: '免费开源，GPL-3.0',
    installs: '{n}+ 次安装',
  },

  open: {
    h: '只要是视频，都能打开',
    lede:
      '文件、文件夹、链接、磁力链接、`.torrent` 文件以及资源的 RSS 订阅，都在同一个窗口中打开。播放由 libmpv 完成，mpv 能播放的一切，包括 MKV、HEVC、AV1、HDR 和杜比视界，这里都能播放。',
    checks: [
      '**打开一集，整个文件夹就成了播放队列**，按自然顺序排列：第 2 集在第 10 集之前。',
      '**视频网页通过 yt-dlp 打开**，链接第一次需要它时，播放器可以帮你安装。',
      '**重启后也能继续播放**，种子里的某一集也一样。',
    ],
  },

  torrents: {
    h: '边下载，边观看',
    lede:
      '粘贴磁力链接或打开 `.torrent` 文件，Frame Player 会在下载的同时开始播放。播放位置附近的文件块优先下载，进度条会标出已经下载好的部分，跳转之前就知道能不能马上播放。',
    checks: [
      '**打开之前不会下载任何内容**，做种默认关闭。',
      '**资源自带的字幕会自动加载**，下一集会提前下载。',
      '**卡顿时会说明原因：**下载速度、用户数，以及文件已下载了多少。',
    ],
    more: '种子边下边播的原理',
  },

  tv: {
    h: '投屏到电视，无需重新封装',
    lede:
      '一个按钮即可投屏到 Chromecast、DLNA 电视，或者（在 Mac 上）支持 AirPlay 的电视。列表中的每台电视都会说明*这个*文件将如何处理；如果电视能直接播放这个资源（常见的是带杜比音频的 4K HEVC HDR），就完全不会转码。',
    checks: [
      '**否则会重新封装文件**，视频流直接复制而不是重新编码，几秒钟就能准备好。',
      '**种子在下载过程中也能投屏。**',
      '**窗口变成遥控器：**快捷键、章节和播放队列都能控制电视，断开连接后在电脑上继续播放。',
    ],
    more: '投屏详解',
  },

  together: {
    h: '两台电脑，同一条时间轴',
    lede:
      '分享一个六位代码，房间里的每个人进度一致、同时暂停、看同一集。一个人跳转，另一个人也会落在同一帧上，网速慢也一样。',
    checks: [
      '**靠微调播放速度纠正偏差，而不是跳转**：以 103% 的速度播放十秒，差距就消失了，画面不会跳动。',
      '**房间会等正在缓冲的人**，然后自动继续。',
      '**种子无需各自粘贴：**大家会自动连接同一个种子的用户。经过中转服务器的只有时间轴，视频从不经过。',
    ],
    more: '一起看的原理',
  },

  subs: {
    h: '一开始就对得上的字幕',
    lede:
      'Frame Player 按文件哈希在 OpenSubtitles 上搜索，找到的是与你手里完全相同的版本，而不是按片名猜测。按片名搜索只是备选，面板会告诉你用的是哪一种。',
    checks: [
      '**下载前就会标出帧率不匹配**：25 fps 的字幕配上 23.976 fps 的文件，看得越久偏差越大。',
      '**控制栏显示时，字幕会移到它上方**，你设置的延迟会为该文件保留。',
      '**第 1 集选的配音，第 2 集会自动再选上**，按语言和音轨名称匹配，而不是按音轨编号。',
    ],
    more: '字幕为什么会不同步',
  },

  catalog: {
    h: '不离开播放器，找到想看的内容',
    lede:
      '浏览海报、分季信息和本地化片名，然后从你自己的 Torznab 索引器（例如 Jackett 或 Prowlarr）中挑选资源，播放器本身不内置任何索引器。资源以种子方式打开，同样支持继续播放和播放队列。',
    checks: [
      '**资源按画质排序**，其次是动态范围，最后是做种数。',
      '**播放器不带任何 API 密钥**：元数据通过项目自己的代理获取。',
      '**这是唯一会发送你搜索内容的功能**，所以它可以关闭，其他功能不依赖它也能正常使用。',
    ],
  },

  resume: {
    h: '无论看什么，都能接着上次看',
    lede:
      '文件、链接和种子里的剧集，都会从你停下的地方继续。开始界面列出所有没看完的内容，每一项都配有取自视频本身的封面帧。',
    checks: [
      '**看完的视频会自动从列表中移除**，种子会记住你看过哪几集。',
      '**一季只占一行，而不是九张卡片**，点击即可继续你正在看的那一集。',
    ],
  },

  picture: {
    h: '画面裁剪与调节，边看边调',
    lede:
      '自动裁剪会测量整个文件中的帧，去掉压制在视频里的黑边，窗口也会随画面改变形状。亮度、对比度、饱和度和伽马实时调节，菜单不会挡住画面。',
    checks: [
      '**一次选择覆盖整季**：每一集单独测量，所以画幅更宽的大结局不会按试播集的黑边被裁掉。',
      '**正在下载的种子也能用。**',
      '**调整可应用于单个文件、它所在的播放列表或所有文件**；两者冲突时，播放器会问你保留哪一个。',
    ],
    more: ['自动裁剪如何找到黑边', '为什么同一个视频在 mpv 里比在 QuickTime 里暗'],
  },

  everything: {
    h: '缺了才会注意到的细节',
    lede: '其他播放器总差那么一点的小地方。',
    cells: [
      { h: '半秒左右就出现在屏幕上', p: '在 Apple Silicon Mac 上，从点击到窗口可用只需 **0.56 秒**：所有东西都在需要时才加载。' },
      { h: '实时媒体信息', p: '编码、分辨率、声道布局和实际码率，面板打开期间每秒刷新。' },
      { h: '窗口记得自己的位置', p: '启动时恢复大小和位置，或者让窗口贴合视频，同时不超出屏幕。' },
      { h: '预览就是你将跳到的那一帧', p: '指针停下后，进度条缩略图会精确到那一帧，而不是它前面的关键帧。' },
      { h: '真正认得片头的“跳过片头”', p: '章节按完整标题匹配，所以“Ending the war for good”永远不会被当成片尾。' },
      { h: '稳稳待在原处的迷你播放器', p: '小巧、始终置顶、贴边吸附到角落。在 macOS 上，它还能悬浮在全屏应用之上。' },
      { h: '47 个操作，全部可重新绑定', p: '快捷键对应物理按键，任何键盘布局下都能用。J、K、L 的作用正如你所料。' },
      { h: 'HDR、杜比视界，什么都行', p: '由 libmpv 解码，与 mpv 和 IINA 相同：完整的格式支持、硬件解码和 HDR 色调映射。' },
      { h: 'A-B 循环与逐帧', p: '循环播放两个标记之间的片段，也能向前或向后逐帧。' },
      { h: '缩放和平移不跳动', p: '缩放和位置在同一帧里改变，画面不会在两次重绘之间跳动。' },
      { h: '保存当前帧', p: '保存或复制解码出的那一帧本身，HDR 色调映射也一并保留。' },
      { h: '省心的音频', p: '选择输出设备，让忽轻忽响的电影音量均衡，并为每个文件单独保留音频延迟。' },
      { h: '文件夹就是播放队列', p: '下一集会在可取消的倒计时后开始。可以单个文件循环、整个队列循环，或者不循环。' },
      { h: '不留痕迹的文件夹', p: '把文件夹设为私密，它的任何信息都不会被记录或发送到任何地方。没有账号，播放器只报告一件事，而且只报告一次：它被安装了。' },
      { h: '你的 mpv.conf 依然说了算', p: '设置只修改 mpv.conf 中属于它自己的那几行，你手写的选项都会保留。' },
    ],
    spare: '哎呀，这一格得补上。15 除以 2 除不尽，所以在这个宽度下网格会少一格。把窗口拉宽，这个角就会消失。',
  },

  faq: {
    h: '常见问题',
    items: [
      {
        q: 'Frame Player 是什么？',
        a: 'Frame Player 是一款基于 libmpv 的免费开源视频播放器，适用于 Windows 10、Windows 11 和 Apple Silicon Mac。它可以播放本地文件、链接和种子，种子可以边下边播，能投屏到 Chromecast 和 DLNA 电视，还能让多人在同一条时间轴上一起看。',
      },
      {
        q: 'Frame Player 免费吗？',
        a: '免费。Frame Player 以 GPL-3.0 许可证免费开源，无需账号，没有订阅，也没有广告。',
      },
      {
        q: 'Frame Player 能在种子下载完之前播放吗？',
        a: '能。粘贴磁力链接或打开 .torrent 文件，下载的同时就开始播放，播放位置附近的文件块会优先下载。做种默认关闭。',
        more: { href: '/torrent-streaming', label: '种子边下边播的原理' },
      },
      {
        q: 'Frame Player 能投屏到电视吗？',
        a: '能。可以投屏到 Chromecast 等 Google Cast 设备和 DLNA 电视，在 Mac 上还能投到 Apple TV 或支持 AirPlay 的电视。如果电视能直接播放文件，Frame Player 会不经转码直接发送；否则会重新封装文件，并直接复制视频流。',
        more: { href: '/cast-to-tv', label: '投屏到电视' },
      },
      {
        q: 'Frame Player 和 VLC、IINA 有什么不同？',
        a: 'Frame Player 使用 libmpv（mpv 和 IINA 背后的引擎）解码，同时支持 Windows 和 macOS，而 IINA 只支持 macOS。它内置了种子边下边播和一起看，VLC 和 IINA 都没有；它投屏到 Chromecast 和 DLNA 电视时无需转码，而 VLC 只能投屏到 Chromecast，并且要实时转码，IINA 则不支持投屏。',
        more: { href: '/compare/vlc', label: 'Frame Player 与 VLC 对比' },
      },
      {
        q: 'Frame Player 会收集数据吗？',
        a: '没有跟踪，也没有账号。除了检查更新和你自己使用的服务（例如搜索字幕），Frame Player 只会在首次启动时发送一次匿名报告，其中只包含平台信息。',
      },
      {
        q: 'Frame Player 能在 Linux 或 Intel Mac 上运行吗？',
        a: '暂时不能。Frame Player 目前适用于 Windows 10 和 11（x64 和 ARM 处理器均可）以及 Apple Silicon Mac。',
      },
    ],
  },

  recent: {
    h: '最近更新',
    lede: '版本更新频繁，并且会自动安装。以下是最近几个版本带来的变化。',
    andMore: '另有 {n} 项',
    all: '所有版本',
  },

  final: {
    title: ['随便打开点什么。', '下次从你停下的地方接着看。'],
    copy: '复制',
    copied: '已复制',
    copyWhat: '安装命令',
    notarized: 'macOS 版已通过公证',
    unsigned: 'Windows 版未签名，SmartScreen 会拦截一次',
  },

  footer: {
    elsewhere: '相关链接',
    source: '源代码',
    updates: '更新',
    docs: '文档',
    licence: '许可证',
    guides: '指南',
    built: '基于 libmpv、FFmpeg 和 librqbit',
    made: '{author} 用 {heart} 制作',
    love: '爱',
  },

  article: {
    breadcrumb: '面包屑导航',
    updated: '更新于 {date}',
    questions: '常见问题',
    readNext: '继续阅读',
    kind: { feature: '功能', guide: '指南', comparison: '对比' },
  },

  updates: {
    title: '更新记录：Frame Player 的所有版本',
    description: 'Frame Player 每个版本的变化，从最新开始：适用于 Windows 和 macOS 的新功能、改进和修复。',
    feedTitle: 'Frame Player 更新',
    h: '更新记录',
    lede: '每个版本的变化，从最新开始。播放器会自动安装更新，所以这里主要记录已经送达的内容。',
    latest: '最新版本：{version}，{date}',
    feed: 'Atom 订阅',
    english: '版本说明以英文撰写。',
  },

  notFound: {
    title: '页面不存在 · Frame Player',
    description: '此地址没有任何内容。返回首页，或者下载 Frame Player。',
    h: '跳过头了。',
    lede: '这个地址上一帧也没有。链接可能已经过期、拼写有误，或者指向的内容已经移走了。',
    back: '返回首页',
    download: '下载 Frame Player',
    osd: '没有可播放的内容',
    osdSub: '404：无法打开此地址',
    osdPath: '404：无法打开 /{path}',
    subline: '你确定地址没错吗？',
    skip: '跳到首页',
  },

  llms: {
    summary: [
      '基于 libmpv 的免费开源视频播放器，适用于 Windows 和 macOS。它可以播放本地文件、',
      '链接和磁力链接种子，种子边下边播，无需转码即可把文件投屏到 Chromecast 或 DLNA 电视，',
      '还能让房间里的多人在同一条时间轴上一起看。',
    ],
    version: '版本 {version}{released}。许可证：GPL-3.0-or-later。',
    released: '，发布于 {date}',
    platforms: 'Windows 10 和 11（x64 和 ARM64），Apple Silicon 上的 macOS。无需账号，没有跟踪；首次启动时发送一次匿名报告。',
    start: '从这里开始',
    home: '[首页]({url})：这款播放器是什么，以及所有功能的简介。',
    updates: '[更新记录]({url})：每个版本的变化，从最新开始（英文）。',
    downloads: '[下载 Windows 版]({windows}) · [Windows ARM 版]({windowsArm}) · [下载 macOS 版]({macos})：始终是当前版本。',
    features: '功能',
    comparisons: '对比',
    guides: '指南',
    project: '项目',
    source: '[源代码]({url})：Rust、Svelte 和 Go，GPL-3.0-or-later。',
    docs: '[文档]({url})：播放器的内部原理及其原因（英文）。',
    releases: '[发布版本]({url})：每个版本的安装包，说明与 /updates 相同。',
    notes: '说明',
    markdown: '这里的每个页面也提供 Markdown 版本：在地址后加 `.md`，或请求 `text/markdown`。',
    dated: '每个页面都标有最后一次对照播放器核对的日期；引用页面时请一并引用该日期。',
    languages: '其他语言',
    language: '[{name}]({url})',
  },

  markdown: {
    figure: '图：{caption}',
    diagram: '示意图：{caption}',
    note: '注意：',
    source: '来源：{url}',
    updated: '更新于：{date}',
    questions: '常见问题',
  },

  pieceMap: {
    label:
      '一排 48 个文件块。播放位置之前及其后一小段都已在磁盘上；接下来的文件块正在优先下载，再后面的在排队，更远处有一段是之前跳转时下载的，其余的还没有请求。',
    playhead: '播放位置',
    have: '已在磁盘上',
    next: '优先下载',
    soon: '排队等待',
    jump: '之前跳转时下载的',
    none: '尚未请求',
  },

  mock: {
    common: {
      thisFile: '此文件',
      playlist: '播放列表',
      addFile: '添加文件…',
      left: '剩余 {t}',
      peers: '{n} 个用户',
      download: '下载',
    },
    opener: {
      title: '打开链接',
      prompt: '粘贴链接、磁力链接或 RSS 订阅',
      go: '打开',
    },
    torrent: {
      show: 'Orbital Night',
      season: '第 2 季',
      watched: '已看完',
      seedingOff: '做种已关闭',
    },
    cast: {
      title: '在电视上播放',
      rooms: ['客厅', '厨房', '卧室'],
      asIs: '直接播放',
      prepared: '播放前需先准备',
      checking: '正在检查此设备的连接方式…',
    },
    room: {
      code: '代码',
      seek: '跳转',
      sameFrame: '两边停在同一帧',
      inSync: '已同步',
      you: '你',
      youInitial: '你',
      friend: '小安',
      friendInitials: '安',
      host: '房主',
      joined: '已加入',
      laptop: '笔记本',
      desktop: '台式机',
    },
    subs: {
      line: '你在那里看到的一切，都比你脚下的土地还要古老。',
      title: '字幕',
      off: '关闭',
      find: '搜索字幕…',
      dialog: '搜索字幕',
      search: '搜索',
      exactHint: '按文件本身匹配：同一版本，时间轴无需调整即可使用。',
      downloads: '下载 {n} 次',
      exact: '完全匹配',
    },
    player: {
      skipIntro: '跳过片头',
      audioTrack: '音轨',
      tracks: ['英语', '英语（5.1）', '俄语（配音）'],
      delay: '延迟',
      reset: '重置',
      chapter: '第 2 章',
      chapterName: '晨昏线',
      chapterCamp: '湖畔营地',
    },
    picture: {
      looking: '正在检测黑边…',
      crop: '裁剪',
      auto: '自动',
      none: '无',
      color: '亮度与色彩',
      brightness: '亮度',
      contrast: '对比度',
      saturation: '饱和度',
      gamma: '伽马',
      applyTo: '应用于',
      allFiles: '所有文件',
    },
    upscale: {
      title: '超分辨率',
      off: '关闭',
      video: '视频',
      anime: '动画',
      strong: '强化',
      active: '已启用：画面放大到原尺寸的 2.2 倍',
    },
    catalog: {
      title: '片库',
      posters: ['尘线', '雨季', '夜班', '渡口'],
      year: '{title}（{year}）',
      dubs: '2 种配音',
    },
    start: {
      hint: '将视频文件拖到此处，或点击按钮打开',
      openFile: '打开文件',
      openLink: '打开链接',
      catalog: '片库',
      continue: '继续观看',
      torrents: '种子',
      link: '链接',
      longNight: '漫漫长夜',
      highway: '夜色公路（2023）',
      totality: '日全食全程',
      videos: '9 个视频',
      onDisk: '磁盘上 7.4 GB',
      continueAt: '继续：S02E03，剩余 23:14',
    },
  },

  alt: {
    orionMoon: '阳光下的飞船，远处是月球',
    highwayTrails: '沙漠公路上空的星轨',
    highwayRoad: '夜晚沙漠道路上空的星轨',
    lakeGalaxy: '银河倒映在高山湖泊中',
    lakeTrails: '高山湖泊上空旋转的星轨',
    lakeTent: '高山湖畔一顶亮着灯的帐篷',
    earthAurora: '从轨道上看到的极光下的夜晚地球',
    cometSea: '平静夜海上空低垂的彗星',
    ridgeHiker: '漆黑山脊上银河下的徒步者',
    surfGlow: '夜晚海滩上发光的浪花',
    moonEclipse: '漆黑山脊上方一轮暗红色的月食',
  },
}

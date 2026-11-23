import { defineConfig, type DefaultTheme } from 'vitepress'

const base = '/Ludonimbus/'
const repo = 'https://github.com/Shupian0510/Ludonimbus'
const releases = `${repo}/releases/latest`

function sidebarZh(): DefaultTheme.SidebarItem[] {
  return [
    {
      text: '入门',
      items: [
        { text: 'Ludonimbus 是什么', link: '/guide/what-is-ludonimbus' },
        { text: '下载与安装', link: '/guide/install' },
        { text: '首次使用引导', link: '/guide/first-run' },
        { text: '快速上手', link: '/guide/quick-start' },
      ],
    },
    {
      text: '云盘',
      items: [
        { text: '怎么选', link: '/clouds/' },
        { text: 'Google Drive', link: '/clouds/google-drive' },
        { text: '夸克网盘（实验性）', link: '/clouds/quark' },
        { text: '百度网盘（审核中）', link: '/clouds/baidu' },
        { text: 'WebDAV：坚果云、NAS、OpenList', link: '/clouds/webdav' },
        { text: 'S3 兼容存储', link: '/clouds/s3' },
        { text: '本地 / 同步文件夹', link: '/clouds/folder' },
        { text: '换云盘与复制存档', link: '/clouds/switching' },
      ],
    },
    {
      text: '添加游戏',
      items: [
        { text: '扫描本机', link: '/games/scan' },
        { text: '找存档向导', link: '/games/save-finder' },
        { text: '扫描文件夹', link: '/games/scan-folder' },
        { text: '手动添加', link: '/games/manual' },
        { text: '游戏位置变了', link: '/games/moved' },
      ],
    },
    {
      text: 'Steam',
      items: [
        { text: 'Steam 自动同步', link: '/steam/auto-sync' },
        { text: '一键加入 Steam', link: '/steam/add-to-steam' },
        { text: '封面与 SteamGridDB', link: '/steam/artwork' },
        { text: '在 Steam 里显示云存档状态', link: '/steam/status-in-steam' },
      ],
    },
    {
      text: '同步与存档',
      items: [
        { text: '同步怎样工作', link: '/sync/how-it-works' },
        { text: '版本历史', link: '/sync/versions' },
        { text: '存档冲突', link: '/sync/conflicts' },
        { text: '离线游玩', link: '/sync/offline' },
        { text: '多台设备', link: '/sync/devices' },
      ],
    },
    {
      text: '支持的游戏',
      items: [
        { text: '模拟器', link: '/supported/emulators' },
        { text: '重编译 / 移植游戏', link: '/supported/recomps' },
        { text: 'PC 游戏（Ludusavi）', link: '/supported/pc-games' },
      ],
    },
    {
      text: '参考',
      items: [
        { text: '设置项说明', link: '/reference/settings' },
        { text: '常见问题与故障排查', link: '/reference/faq' },
        { text: '隐私政策', link: '/privacy' },
        { text: '更新日志', link: '/reference/changelog' },
        { text: '参与贡献', link: '/reference/contributing' },
      ],
    },
  ]
}

function sidebarEn(): DefaultTheme.SidebarItem[] {
  return [
    {
      text: 'Getting started',
      items: [
        { text: 'What is Ludonimbus', link: '/en/guide/what-is-ludonimbus' },
        { text: 'Download and install', link: '/en/guide/install' },
        { text: 'First-run guide', link: '/en/guide/first-run' },
        { text: 'Quick start', link: '/en/guide/quick-start' },
      ],
    },
    {
      text: 'Clouds',
      items: [
        { text: 'Choosing a cloud', link: '/en/clouds/' },
        { text: 'Google Drive', link: '/en/clouds/google-drive' },
        { text: 'Quark (experimental)', link: '/en/clouds/quark' },
        { text: 'Baidu Netdisk (in review)', link: '/en/clouds/baidu' },
        { text: 'WebDAV: NAS, Nextcloud, OpenList', link: '/en/clouds/webdav' },
        { text: 'S3-compatible storage', link: '/en/clouds/s3' },
        { text: 'Local / synced folder', link: '/en/clouds/folder' },
        { text: 'Switching clouds', link: '/en/clouds/switching' },
      ],
    },
    {
      text: 'Adding games',
      items: [
        { text: 'Scan this PC', link: '/en/games/scan' },
        { text: 'Save finder', link: '/en/games/save-finder' },
        { text: 'Scan folder', link: '/en/games/scan-folder' },
        { text: 'Add manually', link: '/en/games/manual' },
        { text: 'When a game moves', link: '/en/games/moved' },
      ],
    },
    {
      text: 'Steam',
      items: [
        { text: 'Steam auto-sync', link: '/en/steam/auto-sync' },
        { text: 'Add to Steam', link: '/en/steam/add-to-steam' },
        { text: 'Artwork and SteamGridDB', link: '/en/steam/artwork' },
        { text: 'Cloud status inside Steam', link: '/en/steam/status-in-steam' },
      ],
    },
    {
      text: 'Sync and saves',
      items: [
        { text: 'How sync works', link: '/en/sync/how-it-works' },
        { text: 'Version history', link: '/en/sync/versions' },
        { text: 'Conflicts', link: '/en/sync/conflicts' },
        { text: 'Playing offline', link: '/en/sync/offline' },
        { text: 'Multiple devices', link: '/en/sync/devices' },
      ],
    },
    {
      text: 'Supported games',
      items: [
        { text: 'Emulators', link: '/en/supported/emulators' },
        { text: 'Recomps and ports', link: '/en/supported/recomps' },
        { text: 'PC games (Ludusavi)', link: '/en/supported/pc-games' },
      ],
    },
    {
      text: 'Reference',
      items: [
        { text: 'Settings', link: '/en/reference/settings' },
        { text: 'FAQ and troubleshooting', link: '/en/reference/faq' },
        { text: 'Privacy policy', link: '/en/privacy' },
        { text: 'Changelog', link: '/en/reference/changelog' },
        { text: 'Contributing', link: '/en/reference/contributing' },
      ],
    },
  ]
}

export default defineConfig({
  base,
  title: 'Ludonimbus',
  // Pages are served as .html (privacy.html must keep its address: Google
  // OAuth and Baidu's review link to it).
  cleanUrls: false,
  srcExclude: ['README.md', 'SCREENSHOTS.md', 'parts/**'],
  head: [
    ['link', { rel: 'icon', href: `${base}app-icon.svg`, type: 'image/svg+xml' }],
    ['meta', { name: 'theme-color', content: '#1a9fff' }],
  ],

  locales: {
    root: {
      label: '简体中文',
      lang: 'zh-CN',
      description: '给 Steam 里的非 Steam 游戏补上云存档：模拟器、重制版、第三方 PC 游戏，像 Steam 云存档一样自动同步。',
      themeConfig: {
        nav: [
          { text: '指南', link: '/guide/what-is-ludonimbus', activeMatch: '^/(guide|games|steam|sync)/' },
          { text: '云盘', link: '/clouds/', activeMatch: '^/clouds/' },
          { text: '支持的游戏', link: '/supported/emulators', activeMatch: '^/supported/' },
          { text: '常见问题', link: '/reference/faq', activeMatch: '^/reference/' },
          { text: '下载', link: releases },
        ],
        sidebar: { '/': sidebarZh() },
        editLink: { pattern: `${repo}/edit/main/website/:path`, text: '在 GitHub 上改进这一页' },
        footer: {
          message: `<a href="${base}privacy.html">隐私政策</a> · <a href="${repo}">GitHub</a> · <a href="${repo}/issues">反馈问题</a> · MIT 许可`,
          copyright: '© 2026 Shupian0510',
        },
        outline: { label: '本页内容', level: [2, 3] },
        docFooter: { prev: '上一页', next: '下一页' },
        darkModeSwitchLabel: '外观',
        lightModeSwitchTitle: '切换到浅色模式',
        darkModeSwitchTitle: '切换到深色模式',
        sidebarMenuLabel: '目录',
        returnToTopLabel: '回到顶部',
        langMenuLabel: '切换语言',
        notFound: {
          title: '页面不存在',
          quote: '这个地址没有内容，可能是链接写错了或者页面换了位置。',
          linkLabel: '回到首页',
          linkText: '回到首页',
        },
      },
    },
    en: {
      label: 'English',
      lang: 'en-US',
      link: '/en/',
      description: 'Steam-Cloud-like save sync for non-Steam games: emulators, recompilations and third-party PC games you added to Steam.',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/en/guide/what-is-ludonimbus', activeMatch: '^/en/(guide|games|steam|sync)/' },
          { text: 'Clouds', link: '/en/clouds/', activeMatch: '^/en/clouds/' },
          { text: 'Supported games', link: '/en/supported/emulators', activeMatch: '^/en/supported/' },
          { text: 'FAQ', link: '/en/reference/faq', activeMatch: '^/en/reference/' },
          { text: 'Download', link: releases },
        ],
        sidebar: { '/en/': sidebarEn() },
        editLink: { pattern: `${repo}/edit/main/website/:path`, text: 'Improve this page on GitHub' },
        footer: {
          message: `<a href="${base}en/privacy.html">Privacy policy</a> · <a href="${repo}">GitHub</a> · <a href="${repo}/issues">Report a problem</a> · MIT licensed`,
          copyright: '© 2026 Shupian0510',
        },
        outline: { label: 'On this page', level: [2, 3] },
      },
    },
  },

  themeConfig: {
    logo: '/app-icon.svg',
    socialLinks: [{ icon: 'github', link: repo }],
    search: {
      provider: 'local',
      options: {
        miniSearch: {
          // MiniSearch splits on spaces and punctuation, which leaves Chinese
          // text as one long token. Split words with Intl.Segmenter instead.
          // The function is serialized into the client bundle, so it must not
          // use anything from this module.
          options: {
            tokenize: (text: string) => {
              const words: string[] = []
              const segmenter = new Intl.Segmenter('zh-CN', { granularity: 'word' })
              for (const part of segmenter.segment(text)) {
                if (part.isWordLike) words.push(part.segment)
              }
              return words
            },
          },
          searchOptions: { combineWith: 'AND' },
        },
        locales: {
          root: {
            translations: {
              button: { buttonText: '搜索文档', buttonAriaLabel: '搜索文档' },
              modal: {
                displayDetails: '显示详细列表',
                resetButtonTitle: '清除搜索',
                backButtonTitle: '关闭搜索',
                noResultsText: '没有找到相关内容',
                footer: {
                  selectText: '打开',
                  selectKeyAriaLabel: '回车',
                  navigateText: '切换',
                  navigateUpKeyAriaLabel: '上方向键',
                  navigateDownKeyAriaLabel: '下方向键',
                  closeText: '关闭',
                  closeKeyAriaLabel: 'Esc',
                },
              },
            },
          },
        },
      },
    },
  },
})

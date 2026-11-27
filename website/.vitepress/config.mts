import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig, type DefaultTheme, type HeadConfig } from 'vitepress'

const base = '/Ludonimbus/'
/** Public address of the site, used for canonical links, hreflang, the sitemap and Open Graph. */
const site = `https://shupian0510.github.io${base}`
const repo = 'https://github.com/Shupian0510/Ludonimbus'
const releases = `${repo}/releases/latest`

const descriptionZh =
  '开源的 Steam 云存档替代方案：给非 Steam 游戏、模拟器和重编译游戏自动同步存档，带版本历史和 Steam 集成，存档放在你自己的云盘里。'
const descriptionEn =
  'Open-source Steam Cloud alternative for non-Steam games, emulators and recompiled games: automatic save sync, version history and Steam integration, using your own cloud storage.'

// Site ownership tokens from Google Search Console and Bing Webmaster Tools
// ("HTML tag" verification method: paste only the content="…" value).
// Empty strings add no tag.
const googleSiteVerification = '13I1PFIduYetwaPSPLsT6NntscGMQNRCWshdpX2KB30'
const bingSiteVerification = ''

/** The app's current version, shown in the structured data of the home pages. */
function appVersion(): string | undefined {
  const file = fileURLToPath(new URL('../../app/src-tauri/tauri.conf.json', import.meta.url))
  if (!existsSync(file)) return undefined
  return JSON.parse(readFileSync(file, 'utf8')).version
}

/** Public address of a page from its source path: 'en/guide/install.md' → …/en/guide/install.html, 'en/comparison/index.md' → …/en/comparison/. */
function pageUrl(relativePath: string): string {
  return site + relativePath.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '.html')
}

/** The same page in the other language (Chinese at the root, English under en/, same file names). */
function otherLanguage(relativePath: string): string {
  return relativePath.startsWith('en/') ? relativePath.slice(3) : `en/${relativePath}`
}

/** schema.org description of the app, for search engines. */
function softwareApplication(en: boolean) {
  return {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: 'Ludonimbus',
    description: en ? descriptionEn : descriptionZh,
    url: en ? `${site}en/` : site,
    inLanguage: en ? 'en' : 'zh-CN',
    applicationCategory: 'UtilitiesApplication',
    applicationSubCategory: en ? 'Game save sync' : '游戏存档同步',
    operatingSystem: 'Windows 10, Windows 11',
    softwareVersion: appVersion(),
    downloadUrl: releases,
    installUrl: releases,
    screenshot: `${site}screenshots/${en ? 'en' : 'zh-CN'}/library-grid.png`,
    image: `${site}app-icon.svg`,
    license: 'https://opensource.org/licenses/MIT',
    isAccessibleForFree: true,
    offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
    author: { '@type': 'Person', name: 'Shupian0510', url: 'https://github.com/Shupian0510' },
    codeRepository: repo,
    sameAs: [repo],
  }
}

function useCasesZh(): DefaultTheme.SidebarItem {
  return {
    text: '使用场景',
    items: [
      { text: '非 Steam 游戏云存档', link: '/non-steam-cloud-saves/' },
      { text: '模拟器存档同步', link: '/emulator-cloud-saves/' },
      { text: '重编译游戏云存档', link: '/recompiled-game-saves/' },
      { text: 'Steam 云存档插件', link: '/steam-integration/' },
      { text: '用 Google Drive 同步存档', link: '/google-drive-save-sync/' },
      { text: '支持的游戏和模拟器', link: '/supported-games/' },
      { text: '和其他工具对比', link: '/comparison/' },
    ],
  }
}

function useCasesEn(): DefaultTheme.SidebarItem {
  return {
    text: 'Use cases',
    items: [
      { text: 'Cloud saves for non-Steam games', link: '/en/non-steam-cloud-saves/' },
      { text: 'Emulator save sync', link: '/en/emulator-cloud-saves/' },
      { text: 'Recompiled game saves', link: '/en/recompiled-game-saves/' },
      { text: 'Steam integration', link: '/en/steam-integration/' },
      { text: 'Google Drive save sync', link: '/en/google-drive-save-sync/' },
      { text: 'Supported games and emulators', link: '/en/supported-games/' },
      { text: 'Comparison with other tools', link: '/en/comparison/' },
    ],
  }
}

function sidebarZh(): DefaultTheme.SidebarItem[] {
  return [
    useCasesZh(),
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
        { text: '百度网盘', link: '/clouds/baidu' },
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
        { text: '游戏来源与分组', link: '/games/sources' },
      ],
    },
    {
      text: 'Steam',
      items: [
        { text: 'Steam 自动同步', link: '/steam/auto-sync' },
        { text: '一键加入 Steam', link: '/steam/add-to-steam' },
        { text: '封面与 SteamGridDB', link: '/steam/artwork' },
        { text: '在 Steam 里显示云存档状态', link: '/steam/status-in-steam' },
        { text: 'Steam 装的游戏', link: '/steam/steam-cloud' },
      ],
    },
    {
      text: '不只是存档',
      items: [
        { text: '游戏时间', link: '/features/playtime' },
        { text: '截图同步', link: '/features/screenshots' },
        { text: '成就', link: '/features/achievements' },
        { text: '游戏本体备份', link: '/features/game-backup' },
      ],
    },
    {
      text: '同步与存档',
      items: [
        { text: '同步怎样工作', link: '/sync/how-it-works' },
        { text: '存档', link: '/sync/versions' },
        { text: '同一个游戏的几个版本', link: '/sync/game-versions' },
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
      text: '其他平台',
      items: [
        { text: '掌机和手柄界面', link: '/platforms/handheld' },
        { text: 'Linux 和 Steam Deck（预览）', link: '/platforms/linux' },
        { text: 'Switch 主机（预览）', link: '/platforms/switch' },
      ],
    },
    {
      text: '参考',
      items: [
        { text: '设置项说明', link: '/reference/settings' },
        { text: '常见问题与故障排查', link: '/reference/faq' },
        { text: '隐私政策', link: '/privacy' },
        { text: '服务条款', link: '/terms' },
        { text: '更新日志', link: '/reference/changelog' },
        { text: '参与贡献', link: '/reference/contributing' },
      ],
    },
  ]
}

function sidebarEn(): DefaultTheme.SidebarItem[] {
  return [
    useCasesEn(),
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
        { text: 'Baidu Netdisk', link: '/en/clouds/baidu' },
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
        { text: 'Game sources and groups', link: '/en/games/sources' },
      ],
    },
    {
      text: 'Steam',
      items: [
        { text: 'Steam auto-sync', link: '/en/steam/auto-sync' },
        { text: 'Add to Steam', link: '/en/steam/add-to-steam' },
        { text: 'Artwork and SteamGridDB', link: '/en/steam/artwork' },
        { text: 'Cloud status inside Steam', link: '/en/steam/status-in-steam' },
        { text: 'Games Steam installed', link: '/en/steam/steam-cloud' },
      ],
    },
    {
      text: 'Beyond saves',
      items: [
        { text: 'Play time', link: '/en/features/playtime' },
        { text: 'Screenshot sync', link: '/en/features/screenshots' },
        { text: 'Achievements', link: '/en/features/achievements' },
        { text: 'Game backup', link: '/en/features/game-backup' },
      ],
    },
    {
      text: 'Sync and saves',
      items: [
        { text: 'How sync works', link: '/en/sync/how-it-works' },
        { text: 'Saves', link: '/en/sync/versions' },
        { text: 'Several versions of one game', link: '/en/sync/game-versions' },
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
      text: 'Other platforms',
      items: [
        { text: 'Handhelds and the controller view', link: '/en/platforms/handheld' },
        { text: 'Linux and Steam Deck (preview)', link: '/en/platforms/linux' },
        { text: 'Switch console (preview)', link: '/en/platforms/switch' },
      ],
    },
    {
      text: 'Reference',
      items: [
        { text: 'Settings', link: '/en/reference/settings' },
        { text: 'FAQ and troubleshooting', link: '/en/reference/faq' },
        { text: 'Privacy policy', link: '/en/privacy' },
        { text: 'Terms of service', link: '/en/terms' },
        { text: 'Changelog', link: '/en/reference/changelog' },
        { text: 'Contributing', link: '/en/reference/contributing' },
      ],
    },
  ]
}

export default defineConfig({
  base,
  title: 'Ludonimbus',
  // Pages are served as .html (privacy.html must keep its address: the
  // Google OAuth consent screen links to it).
  cleanUrls: false,
  srcExclude: ['README.md', 'SCREENSHOTS.md', 'parts/**'],
  head: [
    ['link', { rel: 'icon', href: `${base}app-icon.svg`, type: 'image/svg+xml' }],
    ['meta', { name: 'theme-color', content: '#1a9fff' }],
    ...(googleSiteVerification
      ? [['meta', { name: 'google-site-verification', content: googleSiteVerification }] as HeadConfig]
      : []),
    ...(bingSiteVerification ? [['meta', { name: 'msvalidate.01', content: bingSiteVerification }] as HeadConfig] : []),
  ],

  // Git commit dates: shown at the bottom of each page and used as <lastmod> in the sitemap.
  lastUpdated: true,
  sitemap: { hostname: site },

  // Per-page tags for search engines and link previews: canonical address,
  // the other language's version (hreflang), Open Graph, and structured data
  // on the home pages.
  transformPageData(pageData, { siteConfig }) {
    const path = pageData.relativePath
    if (!path || pageData.isNotFound) return
    const en = path.startsWith('en/')
    const url = pageUrl(path)
    const title = pageData.frontmatter.titleTemplate === false ? pageData.title : `${pageData.title} | Ludonimbus`
    const description: string = pageData.frontmatter.description || (en ? descriptionEn : descriptionZh)

    const head: HeadConfig[] = [['link', { rel: 'canonical', href: url }]]
    const other = otherLanguage(path)
    if (siteConfig.pages.includes(other)) {
      const enUrl = pageUrl(en ? path : other)
      head.push(
        ['link', { rel: 'alternate', hreflang: 'zh-CN', href: pageUrl(en ? other : path) }],
        ['link', { rel: 'alternate', hreflang: 'en-US', href: enUrl }],
        ['link', { rel: 'alternate', hreflang: 'x-default', href: enUrl }],
      )
    }
    head.push(
      ['meta', { property: 'og:type', content: 'website' }],
      ['meta', { property: 'og:site_name', content: 'Ludonimbus' }],
      ['meta', { property: 'og:title', content: title }],
      ['meta', { property: 'og:description', content: description }],
      ['meta', { property: 'og:url', content: url }],
      ['meta', { property: 'og:image', content: `${site}screenshots/${en ? 'en' : 'zh-CN'}/library-grid.png` }],
      ['meta', { property: 'og:locale', content: en ? 'en_US' : 'zh_CN' }],
      ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    )
    if (path === 'index.md' || path === 'en/index.md') {
      head.push(['script', { type: 'application/ld+json' }, JSON.stringify(softwareApplication(en))])
    }
    pageData.frontmatter.head = [...(pageData.frontmatter.head ?? []), ...head]
  },

  locales: {
    root: {
      label: '简体中文',
      lang: 'zh-CN',
      description: descriptionZh,
      themeConfig: {
        nav: [
          { text: '指南', link: '/guide/what-is-ludonimbus', activeMatch: '^/(guide|games|steam|sync)/' },
          {
            text: '使用场景',
            activeMatch:
              '^/(non-steam-cloud-saves|emulator-cloud-saves|recompiled-game-saves|steam-integration|google-drive-save-sync|comparison)/',
            items: useCasesZh().items!.filter((item) => item.link !== '/supported-games/') as DefaultTheme.NavItemWithLink[],
          },
          { text: '云盘', link: '/clouds/', activeMatch: '^/clouds/' },
          { text: '支持的游戏', link: '/supported-games/', activeMatch: '^/(supported|supported-games)/' },
          { text: '常见问题', link: '/reference/faq', activeMatch: '^/reference/' },
          { text: '下载', link: releases },
        ],
        sidebar: { '/': sidebarZh() },
        lastUpdated: { text: '最后更新于' },
        editLink: { pattern: `${repo}/edit/main/website/:path`, text: '在 GitHub 上改进这一页' },
        footer: {
          message: `<a href="${base}privacy.html">隐私政策</a> · <a href="${base}terms.html">服务条款</a> · <a href="${repo}">GitHub</a> · <a href="${repo}/issues">反馈问题</a> · MIT 许可`,
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
      description: descriptionEn,
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/en/guide/what-is-ludonimbus', activeMatch: '^/en/(guide|games|steam|sync)/' },
          {
            text: 'Use cases',
            activeMatch:
              '^/en/(non-steam-cloud-saves|emulator-cloud-saves|recompiled-game-saves|steam-integration|google-drive-save-sync|comparison)/',
            items: useCasesEn().items!.filter((item) => item.link !== '/en/supported-games/') as DefaultTheme.NavItemWithLink[],
          },
          { text: 'Clouds', link: '/en/clouds/', activeMatch: '^/en/clouds/' },
          { text: 'Supported games', link: '/en/supported-games/', activeMatch: '^/en/(supported|supported-games)/' },
          { text: 'FAQ', link: '/en/reference/faq', activeMatch: '^/en/reference/' },
          { text: 'Download', link: releases },
        ],
        sidebar: { '/en/': sidebarEn() },
        lastUpdated: { text: 'Last updated' },
        editLink: { pattern: `${repo}/edit/main/website/:path`, text: 'Improve this page on GitHub' },
        footer: {
          message: `<a href="${base}en/privacy.html">Privacy policy</a> · <a href="${base}en/terms.html">Terms of service</a> · <a href="${repo}">GitHub</a> · <a href="${repo}/issues">Report a problem</a> · MIT licensed`,
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

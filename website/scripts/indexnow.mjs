// Tells IndexNow search engines (Bing, Yandex, Seznam, Naver…) which pages of
// the deployed site changed, so they recrawl them sooner. Google does not use
// IndexNow; it reads the sitemap.
//
//   node scripts/indexnow.mjs <before-sha> <after-sha>   pages whose source changed between two commits
//   node scripts/indexnow.mjs --all                      every page in the sitemap
//
// Run after the site is deployed: it reads the live sitemap, and search
// engines check the key file on the live site. The key is public by design;
// public/<key>.txt holds the same value.

import { execFileSync } from 'node:child_process'

const site = 'https://shupian0510.github.io/Ludonimbus/'
const key = '682015ff8ef9063735475f3d73379694'

/** Every page in the live sitemap. */
async function allPages() {
  // The query string skips the CDN's cached copy from before this deployment.
  const xml = await (await fetch(`${site}sitemap.xml?t=${Date.now()}`)).text()
  const urls = [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => m[1])
  if (urls.length === 0) throw new Error('No pages found in the live sitemap')
  return urls
}

/** Source files changed between two commits, relative to website/, or null when every page should be sent. */
function changedFiles(before, after) {
  if (!before || !after || /^0+$/.test(before)) return null
  try {
    // Paths come back relative to the repository root: website/… and CHANGELOG.md.
    const out = execFileSync('git', ['diff', '--name-only', before, after, '--', '.', '../CHANGELOG.md'], {
      encoding: 'utf8',
    })
    return out.split('\n').filter(Boolean).map((f) => f.replace(/^website\//, ''))
  } catch {
    return null // the old commit is not in this clone (force push, shallow clone…)
  }
}

/** The public address of a page from its source path, the same mapping as pageUrl() in .vitepress/config.mts. */
function pageUrl(path) {
  return site + path.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '.html')
}

const files = process.argv[2] === '--all' ? null : changedFiles(process.argv[2], process.argv[3])
let urls
if (files === null || files.some((f) => f.startsWith('.vitepress/') || f.startsWith('parts/'))) {
  // Config, theme and shared parts can change every page.
  urls = await allPages()
} else {
  // Deleted pages are sent too: that tells search engines to drop them.
  const notPages = new Set(['README.md', 'SCREENSHOTS.md', 'CHANGELOG.md'])
  const wanted = new Set(files.filter((f) => f.endsWith('.md') && !notPages.has(f)).map(pageUrl))
  if (files.includes('CHANGELOG.md')) {
    wanted.add(pageUrl('reference/changelog.md'))
    wanted.add(pageUrl('en/reference/changelog.md'))
  }
  urls = [...wanted]
}

if (urls.length === 0) {
  console.log('IndexNow: no page changed')
  process.exit(0)
}

const res = await fetch('https://api.indexnow.org/indexnow', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json; charset=utf-8' },
  body: JSON.stringify({
    host: new URL(site).host,
    key,
    keyLocation: `${site}${key}.txt`,
    urlList: urls,
  }),
})
console.log(`IndexNow: sent ${urls.length} page(s), HTTP ${res.status}`)
for (const u of urls) console.log(`  ${u}`)
// 200 and 202 mean accepted. Anything else is reported but doesn't fail the
// deployment, which has already happened.
if (res.status !== 200 && res.status !== 202) console.log(await res.text())

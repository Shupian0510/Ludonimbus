// Builds the changelog pages from the repository's CHANGELOG.md.
//
// Every version in CHANGELOG.md has a "### 中文" and a "### English" half;
// the Chinese halves go to reference/changelog.md and the English halves to
// en/reference/changelog.md. Both files are generated (and git-ignored), so
// run this before `vitepress dev` / `vitepress build` (the npm scripts do).

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const site = join(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(site, '..', 'CHANGELOG.md')
const releases = 'https://github.com/Shupian0510/Ludonimbus/releases/tag/v'

const text = readFileSync(source, 'utf8').replace(/\r\n/g, '\n')

/** [{ version, zh, en }] in file order (newest first). */
function parse(md) {
  const out = []
  const parts = md.split(/^## /m).slice(1)
  for (const part of parts) {
    const nl = part.indexOf('\n')
    const version = part.slice(0, nl).trim()
    const body = part.slice(nl + 1)
    const zh = body.match(/^### 中文\s*\n([\s\S]*?)(?=^### |(?![\s\S]))/m)
    const en = body.match(/^### English\s*\n([\s\S]*?)(?=^### |(?![\s\S]))/m)
    if (!zh || !en) throw new Error(`CHANGELOG.md: ${version} needs both "### 中文" and "### English"`)
    out.push({ version, zh: zh[1].trim(), en: en[1].trim() })
  }
  if (out.length === 0) throw new Error('CHANGELOG.md: no versions found')
  return out
}

/** Keep `{{ }}` from being read as Vue interpolation. */
const escape = (s) => s.replace(/\{\{/g, '&#123;&#123;').replace(/\}\}/g, '&#125;&#125;')

function page(entries, lang) {
  const zh = lang === 'zh'
  const head = zh
    ? [
        '# 更新日志',
        '',
        '每个版本的更新内容。下载安装包请到 [GitHub Releases](https://github.com/Shupian0510/Ludonimbus/releases)；已经装了 Ludonimbus 的话，有新版本时应用会提示你一键更新。',
      ]
    : [
        '# Changelog',
        '',
        'What changed in each version. Installers are on [GitHub Releases](https://github.com/Shupian0510/Ludonimbus/releases); if Ludonimbus is already installed, it offers new versions itself.',
      ]
  const lines = ['---', 'editLink: false', '---', '', '<!-- Generated from CHANGELOG.md by scripts/gen-changelog.mjs. Do not edit. -->', '', ...head, '']
  for (const e of entries) {
    lines.push(`## ${e.version}`, '')
    lines.push(escape(zh ? e.zh : e.en), '')
    lines.push(zh ? `[在 GitHub 上查看 ${e.version}](${releases}${e.version})` : `[${e.version} on GitHub](${releases}${e.version})`, '')
  }
  return lines.join('\n')
}

const entries = parse(text)
for (const [lang, file] of [
  ['zh', join(site, 'reference', 'changelog.md')],
  ['en', join(site, 'en', 'reference', 'changelog.md')],
]) {
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, page(entries, lang))
}
console.log(`changelog: ${entries.length} versions (${entries[0].version} … ${entries.at(-1).version})`)

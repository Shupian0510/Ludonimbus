<!-- 目标分支是 dev（发版、hotfix/*、docs/* 才进 main）。 / The base branch is dev (only releases, hotfix/* and docs/* go into main). -->

## 做了什么 / What this does


## 怎么验证的 / How it was checked


## 检查 / Checklist

- [ ] 只做一件事 / One thing only
- [ ] 核心逻辑的改动带测试，测试不碰真实的存档、Steam 配置和云盘 / Tests for core logic, using temporary folders only
- [ ] 用户看得见的改动：`CHANGELOG.md`、界面文字、文档站都更新了中文和英文 / For visible changes: `CHANGELOG.md`, interface texts and the documentation site, in both languages
- [ ] 没有改版本号 / The version number is unchanged

#!/usr/bin/env python3
"""Generate adapters/recomplauncher.yaml from RecompLauncher's game catalog.

    python scripts/import-recomplauncher.py [pc-catalog.json]

Without an argument the catalog is downloaded from
https://recomplauncher.com/pc-catalog.json. Only the standard library is used.

What is taken from each Windows game in the catalog:

- Save locations its developer declared in recomplauncher.json
  (`launcher.saves`, `launcher.folders.saves`).
- Otherwise, for N64Recomp games (N64ModernRuntime keeps saves in
  `<app folder>/saves`, the app folder being `%LOCALAPPDATA%\\<name>` or the
  game folder in portable mode), the folder its settings file is in.
- The program names, to recognise the game from a Steam shortcut.

RecompLauncher's `{install}` is the release folder, which is what Ludonimbus
uses too when the exe sits deeper in it (the match path includes the folders).
"""

from __future__ import annotations

import datetime
import json
import os
import sys
import urllib.request

CATALOG_URL = "https://recomplauncher.com/pc-catalog.json"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "adapters", "recomplauncher.yaml")

CONSOLES = {
    "n64": "N64", "gc": "GameCube", "wii": "Wii", "wiiu": "Wii U", "switch": "Switch", "switch2": "Switch 2",
    "ps1": "PS1", "ps2": "PS2", "ps3": "PS3", "ps4": "PS4", "ps5": "PS5", "psp": "PSP", "vita": "PS Vita",
    "x360": "Xbox 360", "xbox": "Xbox", "xseries": "Xbox Series", "snes": "SNES", "nes": "NES", "gba": "GBA",
    "gb": "Game Boy", "gbc": "Game Boy Color", "ds": "DS", "3ds": "3DS", "genesis": "Mega Drive", "dc": "Dreamcast",
    "saturn": "Saturn", "s32x": "32X", "arcade": "街机", "amiga": "Amiga", "cdi": "CD-i", "vb": "Virtual Boy",
    "gg": "Game Gear", "pc": "PC",
}

# Program names too common to recognise a game by.
GENERIC_EXES = {
    "launcher.exe", "setup.exe", "play.exe", "main.exe", "game.exe", "start.exe", "run.exe", "app.exe",
    "client.exe", "launcher-config.exe", "jugar.exe", "updater.exe", "config.exe",
}

TOKENS = {
    "{install}": "{install}",
    "{appdata}": "{appdata}",   # RecompLauncher: AppData\Roaming on Windows
    "{config}": "{appdata}",    # dirs::config_dir() = AppData\Roaming
    "{local}": "{localappdata}",
    "{documents}": "{documents}",
    "{home}": "{home}",
    "{data}": "{install}/recompdata",  # the folder RecompLauncher gives a game
}


def windows_exes(app: dict) -> list[str]:
    progs: list[str] = []
    start = (app.get("launcher") or {}).get("start") or {}
    p = start.get("program") if isinstance(start, dict) else None
    if isinstance(p, str):
        p = [p]
    progs += list(p or [])
    boot = app.get("boot") or {}
    if boot.get("start"):
        progs.append(boot["start"])
    out: list[str] = []
    for e in progs:
        e = e.replace("\\", "/").lstrip("./")
        if e.lower().endswith(".exe") and e not in out:
            out.append(e)
    return out


def matchable(exe: str) -> bool:
    name = exe.rsplit("/", 1)[-1].lower()
    if "setup" in name or "installer" in name or "uninst" in name:
        return False
    # A generic name is fine when the folders around it are distinctive.
    return name not in GENERIC_EXES or "/" in exe


def declared_saves(app: dict) -> list[str]:
    launcher = app.get("launcher") or {}
    out: list[str] = []
    for t in launcher.get("saves") or []:
        if isinstance(t, str) and t not in out:
            out.append(t)
    for f in (launcher.get("folders") or {}).get("saves") or []:
        t = f.get("path") if isinstance(f, dict) else f
        if isinstance(t, str) and t not in out:
            out.append(t)
    return out


def n64recomp_saves(app: dict) -> list[str]:
    """N64ModernRuntime games: graphics.json sits in the app folder."""
    boot = app.get("boot") or {}
    files: list[str] = []
    for c in ((boot.get("settings") or {}).get("config") or []):
        for f in c.get("file") or []:
            files.append(f.get("path") if isinstance(f, dict) else f)
    files = [f for f in files if isinstance(f, str)]
    if "{install}/graphics.json" not in files:
        return []
    for f in files:
        for tok in ("{local}/", "{config}/"):
            if f.startswith(tok) and f.endswith("/graphics.json") and f.count("/") == 2:
                return [f[: -len("graphics.json")] + "saves", "{install}/saves"]
    return []


def convert(template: str, exes: list[str]) -> str | None:
    t = template.strip().replace("\\", "/").rstrip("/")
    if not t.startswith("{") or "}" not in t:
        return None
    tok = t[: t.index("}") + 1]
    rest = t[len(tok):]
    if tok not in TOKENS or ".." in t.split("/") or "{" in rest:
        return None
    if tok == "{home}" and (rest.startswith("/Library") or rest.startswith("/.")):
        return None  # macOS / Linux places
    out = TOKENS[tok] + rest
    if tok == "{install}":
        lower = out.lower()
        if lower == "{install}" or lower.startswith("{install}/usr/") or lower == "{install}/usr":
            return None  # the whole game, or an AppImage's insides
        # The folder a program is in (or one above it) is the game, not saves.
        for e in exes:
            parts = e.split("/")[:-1]
            for i in range(len(parts) + 1):
                if lower == ("{install}/" + "/".join(parts[:i])).rstrip("/").lower():
                    return None
    if out.lower() == "{install}/recompdata":
        return None
    return out


def yaml_str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def yaml_list(items: list[str]) -> str:
    return "[" + ", ".join(yaml_str(i) for i in items) + "]"


def main() -> None:
    if len(sys.argv) > 1:
        with open(sys.argv[1], encoding="utf-8") as f:
            catalog = json.load(f)
        source = os.path.basename(sys.argv[1])
    else:
        req = urllib.request.Request(CATALOG_URL, headers={"User-Agent": "Ludonimbus adapter import"})
        with urllib.request.urlopen(req, timeout=120) as r:
            catalog = json.loads(r.read().decode("utf-8"))
        source = CATALOG_URL

    apps = [a for a in catalog.get("apps", []) if "windows" in (a.get("systems") or [])]
    apps.sort(key=lambda a: (-(a.get("players") or 0), -(a.get("downloads") or 0), a.get("id", "")))

    entries: list[str] = []
    seen: set[str] = set()
    counts = {"declared": 0, "n64recomp": 0}
    for app in apps:
        key = (app.get("id") or "").strip()
        if not key or key in seen:
            continue
        exes = windows_exes(app)
        raw = declared_saves(app)
        how = "declared"
        if not raw:
            raw = n64recomp_saves(app)
            how = "n64recomp"
        saves: list[str] = []
        for t in raw:
            c = convert(t, exes)
            if c and c not in saves:
                saves.append(c)
        # A folder listed next to the save places inside it is a catch-all
        # (often a whole game folder): the places inside are enough.
        saves = [t for t in saves if not any(s != t and s.lower().startswith(t.lower() + "/") for s in saves)]
        if not saves:
            continue
        seen.add(key)
        counts[how] += 1
        match = [e for e in exes if matchable(e)]
        wait = []
        for e in match:
            n = e.rsplit("/", 1)[-1]
            if n.lower() not in GENERIC_EXES and n not in wait:
                wait.append(n)
        name = (app.get("game") or app.get("name") or key).strip()
        console = CONSOLES.get(app.get("origin") or "", "")
        platform = f"{app.get('name') or key}（{console}）" if console else (app.get("name") or key)
        lines = [
            f"- key: {yaml_str(key)}",
            f"  name: {yaml_str(name)}",
            f"  platform: {yaml_str(platform)}",
            f"  recomplauncher: [{yaml_str(key)}]",
        ]
        if match:
            lines.append(f"  match: {{ exe: {yaml_list(match)} }}")
        lines.append("  saves:")
        lines += [f"    - {yaml_str(s)}" for s in saves]
        if wait:
            lines.append(f"  waitFor: {yaml_list(wait)}")
        lines.append("  skipJunk: true")
        entries.append("\n".join(lines))

    today = datetime.date.today().isoformat()
    header = f"""# 由 scripts/import-recomplauncher.py 生成，不要手动修改。
# 想修正某个游戏：在 builtin.yaml 里写一条同 key 的规则（手写规则优先）。
#
# 来源：RecompLauncher 的游戏目录（{source}，{today}）
#   - 开发者在 recomplauncher.json 里声明的存档位置：{counts['declared']} 个
#   - N64Recomp 游戏（存档在 <应用文件夹>/saves）：{counts['n64recomp']} 个
# RecompLauncher：https://recomplauncher.com/  https://github.com/elripalda/RecompLauncher
#
# {{install}} 是发布包的根目录：exe 在子文件夹里时 match 写的是相对路径。
"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n" + "\n\n".join(entries) + "\n")
    print(f"wrote {len(entries)} rules to {os.path.normpath(OUT)} ({counts})")


if __name__ == "__main__":
    main()

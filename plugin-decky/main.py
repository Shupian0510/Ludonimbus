"""Ludonimbus for Decky Loader: the backend forwards requests to Ludonimbus's
local API (the desktop app's or the headless `ludonimbus-agent`, whichever
wrote `agent.json`), adding its token. Both serve the same API
(crates/core/src/localapi); the endpoints are in docs/steam-plugin.md.

In Game Mode the desktop app doesn't run, so when nothing answers the panel
offers to start the background service (`systemctl --user start
ludonimbus-agent.service`, the unit scripts/install-linux.sh writes)."""

import asyncio
import json
import os
import subprocess
import time
import urllib.error
import urllib.request

import decky

SERVICE = "ludonimbus-agent.service"
# How long a freshly started service has to answer.
START_WAIT_SECS = 15
# `GET /v1/changes` waits up to 25 s for something to change (API 6).
CHANGES_TIMEOUT = 40
TIMEOUT = 20


def _home() -> str:
    return getattr(decky, "DECKY_USER_HOME", None) or os.path.expanduser("~")


def _user() -> str:
    return getattr(decky, "DECKY_USER", None) or os.environ.get("USER") or os.path.basename(_home())


def _agent_file() -> str:
    config = os.environ.get("XDG_CONFIG_HOME") or os.path.join(_home(), ".config")
    return os.path.join(config, "Ludonimbus", "agent.json")


def _agent():
    with open(_agent_file(), "r", encoding="utf-8") as f:
        return json.load(f)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


# Only ever 127.0.0.1: no system proxy (it would see the token) and no
# redirects (they would carry it elsewhere).
_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())


def _call(method: str, path: str):
    """The blocking part of `Plugin.request`, run in a worker thread."""
    try:
        info = _agent()
    except (OSError, ValueError):
        return {"ok": False, "status": 0, "body": {"error": "Ludonimbus is not running"}}
    req = urllib.request.Request(
        f"http://127.0.0.1:{int(info['port'])}{path}",
        method=method,
        data=b"" if method == "POST" else None,
        headers={"Authorization": f"Bearer {info['token']}"},
    )
    timeout = CHANGES_TIMEOUT if path.startswith("/v1/changes") else TIMEOUT
    try:
        with _OPENER.open(req, timeout=timeout) as resp:
            return {"ok": True, "status": resp.status, "body": json.loads(resp.read() or b"null")}
    except urllib.error.HTTPError as e:
        try:
            body = json.loads(e.read() or b"null")
        except ValueError:
            body = {"error": str(e)}
        return {"ok": False, "status": e.code, "body": body}
    except (urllib.error.URLError, OSError, ValueError, KeyError, TypeError) as e:
        return {"ok": False, "status": 0, "body": {"error": str(e)}}


def _systemctl(*args: str):
    """`systemctl --user …` for the Deck's user: (exit code, what it said).
    Decky starts plugin backends without the user's session, so the user's
    runtime folder and session bus are given; a backend running as root
    reaches the user's manager with `--machine`."""
    env = dict(os.environ)
    cmd = ["systemctl", "--user"]
    if os.geteuid() == 0:
        cmd.append(f"--machine={_user()}@")
    else:
        run = f"/run/user/{os.getuid()}"
        env["XDG_RUNTIME_DIR"] = run
        env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={run}/bus"
        env["HOME"] = _home()
    try:
        p = subprocess.run(cmd + list(args), env=env, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.SubprocessError) as e:
        return 1, str(e)
    said = (p.stderr or p.stdout or "").strip().splitlines()
    return p.returncode, said[-1] if said else ""


def _service_installed() -> bool:
    unit = os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.join(_home(), ".config"), "systemd", "user", SERVICE)
    return os.path.isfile(unit)


def _answers() -> bool:
    return _call("GET", "/v1/ping").get("ok", False)


def _start():
    """The blocking part of `Plugin.start_service`."""
    if _answers():
        return {"ok": True, "already": True}
    if not _service_installed():
        return {"ok": False, "error": "notInstalled", "detail": ""}
    code, said = _systemctl("start", SERVICE)
    if code != 0:
        decky.logger.error("systemctl --user start %s: %s", SERVICE, said)
        return {"ok": False, "error": "failed", "detail": said}
    for _ in range(START_WAIT_SECS * 2):
        if _answers():
            return {"ok": True, "already": False}
        time.sleep(0.5)
    _, state = _systemctl("is-active", SERVICE)
    decky.logger.error("%s started but doesn't answer (%s)", SERVICE, state)
    return {"ok": False, "error": "noAnswer", "detail": state}


class Plugin:
    async def request(self, method: str, path: str):
        """Call the local API. Returns {"ok": bool, "status": int, "body": ...}."""
        if method not in ("GET", "POST") or not path.startswith("/v1/"):
            return {"ok": False, "status": 400, "body": {"error": "bad request"}}
        # Off the event loop: a slow answer must not hold up Decky.
        return await asyncio.get_running_loop().run_in_executor(None, _call, method, path)

    async def start_service(self):
        """Start the background service and wait until it answers.
        Returns {"ok": bool, "error": "notInstalled" | "failed" | "noAnswer", "detail": str}."""
        return await asyncio.get_running_loop().run_in_executor(None, _start)

    async def _main(self):
        decky.logger.info("Ludonimbus plugin loaded; agent file: %s", _agent_file())

    async def _unload(self):
        pass


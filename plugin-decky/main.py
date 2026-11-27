"""Ludonimbus for Decky Loader: the backend only forwards requests to
Ludonimbus's local API (the desktop app's or the headless `ludonimbus-agent`,
whichever wrote `agent.json`), adding its token. See crates/agent/src/main.rs
for the endpoints."""

import asyncio
import json
import os
import urllib.error
import urllib.request

import decky


def _agent_file() -> str:
    home = getattr(decky, "DECKY_USER_HOME", None) or os.path.expanduser("~")
    config = os.environ.get("XDG_CONFIG_HOME") or os.path.join(home, ".config")
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
    try:
        with _OPENER.open(req, timeout=20) as resp:
            return {"ok": True, "status": resp.status, "body": json.loads(resp.read() or b"null")}
    except urllib.error.HTTPError as e:
        try:
            body = json.loads(e.read() or b"null")
        except ValueError:
            body = {"error": str(e)}
        return {"ok": False, "status": e.code, "body": body}
    except (urllib.error.URLError, OSError, ValueError, KeyError, TypeError) as e:
        return {"ok": False, "status": 0, "body": {"error": str(e)}}


class Plugin:
    async def request(self, method: str, path: str):
        """Call the local API. Returns {"ok": bool, "status": int, "body": ...}."""
        if method not in ("GET", "POST") or not path.startswith("/v1/"):
            return {"ok": False, "status": 400, "body": {"error": "bad request"}}
        # Off the event loop: a slow answer must not hold up Decky.
        return await asyncio.get_running_loop().run_in_executor(None, _call, method, path)

    async def _main(self):
        decky.logger.info("Ludonimbus plugin loaded; agent file: %s", _agent_file())

    async def _unload(self):
        pass

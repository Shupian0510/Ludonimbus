-- Ludonimbus Steam plugin backend.
--
-- Talks to the Ludonimbus desktop app (running in the tray) through its local
-- API. The app writes the port and a per-launch token to agent.json in its
-- data folder (%APPDATA%\Ludonimbus on Windows, ~/.config/Ludonimbus on
-- Linux); it is read again on every call, so a restarted app is picked up
-- without reloading Steam. The plugin holds no cloud credentials and does no
-- syncing itself.

local http       = require("http")
local json       = require("json")
local logger     = require("logger")
local millennium = require("millennium")

local function set(value)
    return value ~= nil and value ~= ""
end

-- The same folder as `paths::data_dir()` in crates/core: LUDONIMBUS_HOME,
-- else the system's config folder (dirs::config_dir) + "Ludonimbus".
local function data_dir()
    local home = os.getenv("LUDONIMBUS_HOME")
    if set(home) and home:match("%S") then
        return home
    end
    local appdata = os.getenv("APPDATA")
    if set(appdata) then
        return appdata .. "\\Ludonimbus"
    end
    -- Linux: XDG_CONFIG_HOME when it is an absolute path, else ~/.config.
    local xdg = os.getenv("XDG_CONFIG_HOME")
    if set(xdg) and xdg:sub(1, 1) == "/" then
        return xdg .. "/Ludonimbus"
    end
    local user = os.getenv("HOME")
    if set(user) then
        return user .. "/.config/Ludonimbus"
    end
    return nil
end

local function agent_file()
    local dir = data_dir()
    if not dir then
        return nil
    end
    local sep = dir:find("\\", 1, true) and "\\" or "/"
    return dir .. sep .. "agent.json"
end

local function read_agent()
    local path = agent_file()
    if not path then
        return nil
    end
    local f = io.open(path, "rb")
    if not f then
        return nil
    end
    local text = f:read("*a")
    f:close()
    local ok, info = pcall(json.decode, text)
    if not ok or type(info) ~= "table" or not info.port or not info.token then
        return nil
    end
    return info
end

local function failure(code, detail)
    return json.encode({ error = code, detail = detail })
end

-- Returns the response body (JSON text) or a JSON error object.
local function request(method, path)
    local info = read_agent()
    if not info then
        return failure("not_running")
    end
    local url = "http://127.0.0.1:" .. tostring(math.floor(info.port)) .. path
    local res, err = http.request(url, {
        method = method,
        data = method == "POST" and "" or nil,
        headers = { Authorization = "Bearer " .. info.token },
        timeout = 20,
        follow_redirects = false,
        user_agent = "Ludonimbus-Millennium/1",
    })
    if not res then
        -- Usually "connection refused": the app was closed.
        return failure("not_running", err)
    end
    if res.status == 200 then
        return res.body
    end
    local ok, body = pcall(json.decode, res.body or "")
    local message = ok and type(body) == "table" and body.error or nil
    return json.encode({ error = "api", status = res.status, detail = message })
end

local function digits(value)
    local s = tostring(value or "")
    if s:match("^%d+$") then
        return s
    end
    return nil
end

local function key_part(key)
    local s = tostring(key or "")
    if s:match("^[%w%-_%.]+$") then
        return s
    end
    return nil
end

---@ffi
--- Cloud save status of a Steam shortcut, as JSON text.
---@param appid string
---@param account string
---@param refresh boolean
---@return string
function get_status(appid, account, refresh)
    local id = digits(appid)
    if not id then
        return failure("bad_request")
    end
    local query = {}
    local acc = digits(account)
    if acc then
        table.insert(query, "account=" .. acc)
    end
    if refresh then
        table.insert(query, "refresh=1")
    end
    local path = "/v1/steam/apps/" .. id
    if #query > 0 then
        path = path .. "?" .. table.concat(query, "&")
    end
    return request("GET", path)
end

---@ffi
--- Start "sync now" for a game in the background.
---@param key string
---@return string
function sync_now(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("POST", "/v1/games/" .. k .. "/sync")
end

---@ffi
--- Show the game in the Ludonimbus window.
---@param key string
---@return string
function open_game(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("POST", "/v1/games/" .. k .. "/open")
end

---@ffi
--- Playtime over all devices of every linked shortcut, as JSON text.
---@param account string
---@return string
function get_playtimes(account)
    local acc = digits(account)
    return request("GET", "/v1/steam/playtime" .. (acc and ("?account=" .. acc) or ""))
end

---@ffi
--- Version history of a game in the cloud, as JSON text.
---@param key string
---@return string
function list_snapshots(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("GET", "/v1/games/" .. k .. "/snapshots")
end

---@ffi
--- Restore a version of a game's saves in the background.
---@param key string
---@param id string
---@return string
function restore(key, id)
    local k = key_part(key)
    local v = tostring(id or "")
    if not k or not v:match("^%w+$") then
        return failure("bad_request")
    end
    return request("POST", "/v1/games/" .. k .. "/restore/" .. v)
end

---@ffi
--- Game backup state of a game: files missing, newest backup, running restore.
---@param key string
---@return string
function gamebackup_status(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("GET", "/v1/games/" .. k .. "/gamebackup")
end

---@ffi
--- Show the game in Ludonimbus with the restore dialog open.
---@param key string
---@return string
function gamebackup_install(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("POST", "/v1/games/" .. k .. "/gamebackup/install")
end

---@ffi
--- Whether the Ludonimbus app is running, as JSON text.
---@return string
function ping()
    return request("GET", "/v1/ping")
end

-- Achievements -------------------------------------------------------------

---@ffi
--- Achievement progress and list of a Steam shortcut, as JSON text.
---@param appid string
---@param account string
---@return string
function get_achievements(appid, account)
    local id = digits(appid)
    if not id then
        return failure("bad_request")
    end
    local path = "/v1/steam/apps/" .. id .. "/achievements"
    local acc = digits(account)
    if acc then
        path = path .. "?account=" .. acc
    end
    return request("GET", path)
end

---@ffi
--- A local achievement icon as a data URL ({ dataUrl }).
---@param key string
---@param id string
---@return string
function get_achievement_icon(key, id)
    local k = key_part(key)
    local i = key_part(id)
    if not k or not i then
        return failure("bad_request")
    end
    return request("GET", "/v1/games/" .. k .. "/achievements/" .. i .. "/icon")
end

---@ffi
--- The game's icon as a data URL ({ dataUrl }), for achievements without one.
---@param key string
---@return string
function get_game_icon(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("GET", "/v1/games/" .. k .. "/icon")
end

---@ffi
--- New achievement unlocks after the given sequence number.
---@param after string
---@return string
function get_events(after)
    local seq = digits(after) or "0"
    return request("GET", "/v1/achievements/events?after=" .. seq)
end

local function urlencode(value)
    local s = tostring(value or ""):sub(1, 300)
    return (s:gsub("[^%w%-_%.~]", function(c)
        return string.format("%%%02X", string.byte(c))
    end))
end

---@ffi
--- Tell the app whether Steam showed the popup ("steam" or "failed", with why).
---@param seq string
---@param shown string
---@param reason string
---@return string
function ack_event(seq, shown, reason)
    local s = digits(seq)
    if not s then
        return failure("bad_request")
    end
    local how = shown == "steam" and "steam" or "failed"
    return request("POST", "/v1/achievements/events/" .. s .. "/ack?shown=" .. how .. "&reason=" .. urlencode(reason))
end

-- Steam's hands (API 5) ------------------------------------------------------
-- The frontend reads and changes Steam's own settings of a game (Steam Cloud
-- switch, launch options) when the app asks; these forward the asking and
-- the answers. Queries come escaped from the frontend: only URL-safe
-- characters get through.

local function query_part(value)
    local q = tostring(value or "")
    if #q <= 6000 and q:match("^[%w%-_%.~%%=&]*$") then
        return q
    end
    return nil
end

---@ffi
--- Take what the app asks of Steam, and say what this plugin can do.
---@param query string
---@return string
function plugin_poll(query)
    local q = query_part(query)
    if not q then
        return failure("bad_request")
    end
    return request("GET", "/v1/steam/plugin/poll?" .. q)
end

---@ffi
--- Answer a request, with what Steam says about the game now.
---@param id string
---@param query string
---@return string
function plugin_done(id, query)
    local i = digits(id)
    local q = query_part(query)
    if not i or not q then
        return failure("bad_request")
    end
    return request("POST", "/v1/steam/plugin/done/" .. i .. "?" .. q)
end

---@ffi
--- Say what Steam says about a game (its Steam Cloud switch, launch options).
---@param appid string
---@param query string
---@return string
function plugin_report(appid, query)
    local id = digits(appid)
    local q = query_part(query)
    if not id or not q then
        return failure("bad_request")
    end
    return request("POST", "/v1/steam/apps/" .. id .. "/report?" .. q)
end

---@ffi
--- Hand a backed-up Steam game to Ludonimbus: Steam Cloud off, Ludonimbus syncs it.
---@param key string
---@return string
function take_over(key)
    local k = key_part(key)
    if not k then
        return failure("bad_request")
    end
    return request("POST", "/v1/games/" .. k .. "/steam-cloud/take-over")
end

local function on_load()
    logger:info("Ludonimbus plugin loaded (Millennium " .. millennium.version() .. ")")
    millennium.ready()
end

return {
    on_load = on_load,
}

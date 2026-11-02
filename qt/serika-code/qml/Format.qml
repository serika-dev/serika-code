pragma Singleton
import QtQuick

QtObject {

    // Replace {token} placeholders in a translated string.
    function fmt(text, values) {
        var out = text || ""
        for (var key in values)
            out = out.split("{" + key + "}").join(values[key])
        return out
    }

    function str(strings, key) {
        return (strings && strings[key]) ? strings[key] : ""
    }

    // Look up a translated string: Format.t(ipc.strings, "NewChat", { n: 3 })
    function t(strings, key, values) {
        var text = strings ? strings[key] : undefined
        if (text === undefined) return ""
        return values ? fmt(text, values) : text
    }

    // Ask the art host for a size that fits the screen. It ignores parameters it does not know.
    function sizedArt(url, width, height) {
        if (!url) return ""
        return url + (url.indexOf("?") >= 0 ? "&" : "?") + "output=jpg&w=" + width + "&h=" + height + "&fit=cover&q=82"
    }

    // Today / Yesterday / Previous 7 days / Previous 30 days / month / year, newest first.
    function groupLabel(updatedAtSeconds, strings, nowMs) {
        var now = new Date(nowMs)
        var day = 86400000
        var start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
        var ts = updatedAtSeconds * 1000
        if (ts >= start) return str(strings, "Today") || "Today"
        if (ts >= start - day) return str(strings, "Yesterday") || "Yesterday"
        if (ts >= start - 7 * day) return str(strings, "Previous7Days") || "Previous 7 days"
        if (ts >= start - 30 * day) return str(strings, "Previous30Days") || "Previous 30 days"
        var date = new Date(ts)
        if (date.getFullYear() === now.getFullYear())
            return date.toLocaleDateString(Qt.locale(), "MMMM")
        return String(date.getFullYear())
    }

    function groupSessions(sessions, query, strings) {
        var q = (query || "").trim().toLowerCase()
        var rows = []
        var list = (sessions || []).slice()
        list.sort(function (a, b) { return b.updatedAt - a.updatedAt })
        var now = Date.now()
        for (var i = 0; i < list.length; ++i) {
            var s = list[i]
            if (q && (s.title || "").toLowerCase().indexOf(q) < 0)
                continue
            rows.push({ id: s.id, title: s.title || "", group: groupLabel(s.updatedAt, strings, now) })
        }
        return rows
    }

    // "4h 57m", "3d 2h", "12m"
    function countdown(iso) {
        if (!iso) return ""
        var ms = new Date(iso).getTime() - Date.now()
        if (isNaN(ms)) return ""
        if (ms <= 0) return "0m"
        var s = Math.ceil(ms / 1000)
        var d = Math.floor(s / 86400)
        var h = Math.floor((s % 86400) / 3600)
        var m = Math.floor((s % 3600) / 60)
        if (d > 0) return d + "d " + h + "h"
        if (h > 0) return h + "h " + m + "m"
        return Math.max(1, m) + "m"
    }

    function expiry(iso) {
        if (!iso) return ""
        var date = new Date(iso)
        if (isNaN(date.getTime())) return ""
        return date.toLocaleDateString(Qt.locale(), "MMM d, yyyy")
    }

    function greetingKey(hour) {
        if (hour >= 5 && hour < 12) return "GoodMorning"
        if (hour >= 12 && hour < 18) return "GoodAfternoon"
        if (hour >= 18 && hour < 22) return "GoodEvening"
        return "GoodNight"
    }

    // Translated text when the dictionary has the key, otherwise the English fallback.
    function tr(strings, key, fallback, values) {
        var text = (strings && strings[key] !== undefined) ? strings[key] : fallback
        return values ? fmt(text, values) : text
    }

    // Percent without hiding small values: 0 -> "0", 0.16 -> "0.2", 0.04 -> "<0.1", 4.2 -> "4.2", 38.4 -> "38"
    function pct(p) {
        var n = Number(p)
        if (!isFinite(n) || n <= 0) return "0"
        if (n < 0.1) return "<0.1"
        if (n < 10) return (Math.round(n * 10) / 10).toString()
        return Math.round(n).toString()
    }

    // 1536 -> "1.5 KB"
    function bytes(n) {
        n = Number(n)
        if (!isFinite(n) || n <= 0) return "0 B"
        if (n < 1024) return n + " B"
        if (n < 1048576) return (n / 1024).toFixed(n < 10240 ? 1 : 0) + " KB"
        return (n / 1048576).toFixed(1) + " MB"
    }

    // 72022 -> "72K", 45000000 -> "45M"
    function compact(n) {
        n = Number(n)
        if (!isFinite(n) || n < 0) return ""
        if (n >= 1e9) return (n / 1e9).toFixed(n >= 1e10 ? 0 : 1).replace(/\.0$/, "") + "B"
        if (n >= 1e6) return (n / 1e6).toFixed(n >= 1e7 ? 0 : 1).replace(/\.0$/, "") + "M"
        if (n >= 1e3) return (n / 1e3).toFixed(n >= 1e4 ? 0 : 1).replace(/\.0$/, "") + "K"
        return Math.round(n).toString()
    }

    // "72K of 45M tokens"
    function tokens(used, limit) {
        if (used < 0 || limit <= 0) return ""
        return compact(used) + " of " + compact(limit) + " tokens"
    }

    // "now", "5m", "3h", "2d", "4w", "7mo"
    function relTime(seconds) {
        if (!seconds) return ""
        var diff = Math.max(0, Math.floor(Date.now() / 1000 - seconds))
        if (diff < 60) return "now"
        if (diff < 3600) return Math.floor(diff / 60) + "m"
        if (diff < 86400) return Math.floor(diff / 3600) + "h"
        if (diff < 86400 * 7) return Math.floor(diff / 86400) + "d"
        if (diff < 86400 * 30) return Math.floor(diff / (86400 * 7)) + "w"
        return Math.floor(diff / (86400 * 30)) + "mo"
    }

    // Same, from a unix time in the future: "in 3h 20m"
    function untilSeconds(seconds) {
        if (!seconds) return ""
        var diff = Math.floor(seconds - Date.now() / 1000)
        if (diff <= 0) return "now"
        var d = Math.floor(diff / 86400)
        var h = Math.floor((diff % 86400) / 3600)
        var m = Math.floor((diff % 3600) / 60)
        if (d > 0) return d + "d " + h + "h"
        if (h > 0) return h + "h " + m + "m"
        return Math.max(1, m) + "m"
    }

    function clockTime(seconds) {
        if (!seconds) return ""
        return new Date(seconds * 1000).toLocaleTimeString(Qt.locale(), "h:mm AP")
    }

    function shortPath(path) {
        var p = (path || "").replace(/\\/g, "/").replace(/^\/\/\?\//, "")
        var parts = p.split("/").filter(function (s) { return s.length > 0 })
        if (parts.length <= 2) return p
        return "…/" + parts.slice(-2).join("/")
    }

    function samePath(a, b) {
        return (a || "").replace(/\\/g, "/").toLowerCase() === (b || "").replace(/\\/g, "/").toLowerCase()
    }

    // Flat rows for the sidebar: a header per project, then its chats (newest first).
    // `collapsed` and `expanded` are maps keyed by project path.
    function projectRows(projects, sessions, query, collapsed, expanded, limit) {
        var q = (query || "").trim().toLowerCase()
        var byProject = {}
        var list = (sessions || []).slice()
        list.sort(function (a, b) { return b.updatedAt - a.updatedAt })
        for (var i = 0; i < list.length; ++i) {
            var s = list[i]
            if (q && (s.title || "").toLowerCase().indexOf(q) < 0) continue
            var key = s.key || (s.project || "").replace(/\\/g, "/").toLowerCase()
            if (!byProject[key]) byProject[key] = []
            byProject[key].push(s)
        }
        var heads = (projects || []).map(function (p) {
            var key = p.key || (p.path || "").replace(/\\/g, "/").toLowerCase()
            var chats = byProject[key] || []
            var latest = chats.length ? chats[0].updatedAt : (p.lastUsed || 0)
            return { p: p, key: key, chats: chats, latest: latest }
        })
        // Chats from folders that are no longer in the project list still need a home.
        var known = {}
        heads.forEach(function (h) { known[h.key] = true })
        for (var key2 in byProject) {
            if (known[key2]) continue
            var first = byProject[key2][0]
            heads.push({ p: { path: first.project, name: first.project.replace(/\\/g, "/").split("/").pop(), exists: true,
                              sessionCount: byProject[key2].length, current: false },
                         key: key2, chats: byProject[key2], latest: first.updatedAt })
        }
        heads.sort(function (a, b) {
            if (a.p.current !== b.p.current) return a.p.current ? -1 : 1
            return b.latest - a.latest
        })
        var rows = []
        heads.forEach(function (h) {
            if (q && h.chats.length === 0) return
            var path = h.p.path
            var isCollapsed = !q && collapsed && collapsed[path] === true
            rows.push({ kind: "project", path: path, name: h.p.name, exists: h.p.exists !== false, current: h.p.current === true,
                        count: h.chats.length, collapsed: isCollapsed })
            if (isCollapsed) return
            var showAll = q.length > 0 || (expanded && expanded[path] === true)
            var shown = showAll ? h.chats : h.chats.slice(0, limit)
            shown.forEach(function (s) {
                rows.push({ kind: "session", id: s.id, title: s.title || "", updatedAt: s.updatedAt, project: path })
            })
            if (!showAll && h.chats.length > limit)
                rows.push({ kind: "more", path: path, hidden: h.chats.length - limit })
            if (h.chats.length === 0)
                rows.push({ kind: "empty", path: path })
        })
        return rows
    }

    // Label, icon, and color for a tool call, for both Serika's tools and Claude Code / Codex tools.
    function toolMeta(name) {
        var n = (name || "")
        var lower = n.toLowerCase()
        if (lower === "read_file" || lower === "read" || lower === "notebookread") return { label: "Read", icon: "file-text", tint: "info" }
        if (lower === "list_dir" || lower === "list_files" || lower === "ls") return { label: "Listed", icon: "folder", tint: "info" }
        if (lower === "search_text" || lower === "grep" || lower === "ripgrep") return { label: "Searched", icon: "search", tint: "accent" }
        if (lower === "search_files" || lower === "find_files" || lower === "glob") return { label: "Found files", icon: "search", tint: "accent" }
        if (lower === "write_file" || lower === "write") return { label: "Wrote", icon: "edit-2", tint: "success" }
        if (lower === "replace_in_file" || lower === "edit" || lower === "multiedit" || lower === "notebookedit") return { label: "Edited", icon: "edit-2", tint: "success" }
        if (lower === "apply_patch") return { label: "Applied patch", icon: "edit-2", tint: "success" }
        if (lower === "delete_file") return { label: "Deleted", icon: "trash-2", tint: "danger" }
        if (lower === "move_file") return { label: "Moved", icon: "arrow-right", tint: "success" }
        if (lower === "run_command" || lower === "bash" || lower === "powershell" || lower === "bashoutput") return { label: "Ran", icon: "terminal", tint: "warning" }
        if (lower === "git_status") return { label: "Git status", icon: "git-branch", tint: "warning" }
        if (lower === "git_diff") return { label: "Git diff", icon: "git-branch", tint: "warning" }
        if (lower === "git_log") return { label: "Git log", icon: "git-branch", tint: "warning" }
        if (lower === "git_commit") return { label: "Committed", icon: "git-branch", tint: "warning" }
        if (lower === "browser_open") return { label: "Opened", icon: "globe", tint: "info" }
        if (lower === "browser_snapshot") return { label: "Read page", icon: "eye", tint: "info" }
        if (lower === "browser_click") return { label: "Clicked", icon: "globe", tint: "info" }
        if (lower === "browser_type") return { label: "Typed", icon: "globe", tint: "info" }
        if (lower === "browser_press") return { label: "Pressed", icon: "globe", tint: "info" }
        if (lower === "browser_scroll") return { label: "Scrolled", icon: "globe", tint: "info" }
        if (lower === "browser_back") return { label: "Went back", icon: "globe", tint: "info" }
        if (lower === "webfetch") return { label: "Fetched", icon: "globe", tint: "info" }
        if (lower === "websearch" || lower === "web_search") return { label: "Searched the web", icon: "globe", tint: "info" }
        if (lower === "task" || lower === "agent") return { label: "Subagent", icon: "users", tint: "accent" }
        if (lower === "todowrite") return { label: "Updated tasks", icon: "check-square", tint: "accent" }
        if (lower.indexOf(".") > 0) return { label: n, icon: "tool", tint: "dim" }
        return { label: n, icon: "tool", tint: "dim" }
    }

    function tint(name) {
        if (name === "info") return Theme.info
        if (name === "accent") return Theme.accentSecondary
        if (name === "success") return Theme.success
        if (name === "warning") return Theme.warning
        if (name === "danger") return Theme.danger
        return Theme.textSecondary
    }

    function tintSoft(name) {
        var c = tint(name)
        return Qt.rgba(c.r, c.g, c.b, 0.14)
    }

    function steps(reasoning) {
        var lines = (reasoning || "").split("\n")
        var n = 0
        for (var i = 0; i < lines.length; ++i)
            if (lines[i].trim().length > 0) n++
        return Math.max(1, n)
    }

    // Split markdown into prose and fenced code so code gets its own card with a copy button.
    function splitBlocks(md) {
        var blocks = []
        var text = md || ""
        var re = /```([^\n`]*)\n([\s\S]*?)(```|$)/g
        var last = 0
        var match
        while ((match = re.exec(text)) !== null) {
            if (match.index > last) {
                var prose = text.substring(last, match.index)
                if (prose.trim().length > 0) blocks.push({ code: false, text: prose.trim() })
            }
            var body = match[2]
            if (body.charAt(body.length - 1) === "\n") body = body.substring(0, body.length - 1)
            blocks.push({ code: true, lang: match[1].trim(), text: body })
            last = match.index + match[0].length
            if (match[0].length === 0) re.lastIndex++
        }
        if (last < text.length) {
            var tail = text.substring(last)
            if (tail.trim().length > 0) blocks.push({ code: false, text: tail.trim() })
        }
        return blocks
    }

    function escapeHtml(s) {
        return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    }

    function diffHtml(diff, light, mono) {
        var lines = (diff || "").split("\n")
        var out = ""
        for (var i = 0; i < lines.length; ++i) {
            var line = lines[i]
            var color = light ? "#3f3f46" : "#a1a1aa"
            var bg = "transparent"
            if (line.indexOf("@@") === 0) { color = "#a78bfa"; bg = light ? "#1a8b5cf6" : "#148b5cf6" }
            else if (line.charAt(0) === "+") { color = light ? "#047857" : "#6ee7b7"; bg = light ? "#1a10b981" : "#1410b981" }
            else if (line.charAt(0) === "-") { color = light ? "#be123c" : "#fda4af"; bg = light ? "#1af43f5e" : "#14f43f5e" }
            out += "<div style=\"color:" + color + ";background-color:" + bg + ";white-space:pre;\">" +
                   (escapeHtml(line) || "&nbsp;") + "</div>"
        }
        return "<div style=\"font-family:'" + mono + "';font-size:12px;\">" + out + "</div>"
    }

    // The limit in force now for a subscription provider, or null. A window counts while the
    // provider rejected it and its reset is still ahead, so a stale "limited" flag clears itself.
    function providerLimit(provider, nowMs) {
        var usage = provider && provider.usage ? provider.usage : null
        if (!usage) return null
        var now = (nowMs || Date.now()) / 1000
        var windows = usage.windows || []
        var found = null
        for (var i = 0; i < windows.length; ++i) {
            var w = windows[i]
            var reset = Number(w.resets_at || 0)
            var over = w.status === "rejected" || Number(w.used) >= 1
            if (over && (reset === 0 || reset > now) && (!found || reset > found.resetsAt))
                found = { name: provider.name || "", label: w.label || w.key || "", resetsAt: reset }
        }
        if (!found && usage.limited === true && windows.length === 0 && now - Number(usage.updated_at || 0) < 5 * 3600)
            found = { name: provider.name || "", label: "", resetsAt: 0 }
        return found
    }

    // Serika's own allowance: the weekly or the session window is used up.
    function serikaLimit(weeklyPercent, weeklyResetsAt, sessionPercent, sessionResetsAt) {
        if (weeklyPercent >= 100)
            return { name: "Serika", label: "Weekly", resetsAt: weeklyResetsAt ? new Date(weeklyResetsAt).getTime() / 1000 : 0 }
        if (sessionPercent >= 100)
            return { name: "Serika", label: "Session", resetsAt: sessionResetsAt ? new Date(sessionResetsAt).getTime() / 1000 : 0 }
        return null
    }

    // "Claude Code · Weekly limit reached. Resets in 3d 2h."
    function limitText(limit, strings) {
        if (!limit) return ""
        var what = limit.label ? limit.label + " " + (tr(strings, "LimitWord", "limit")) : tr(strings, "UsageLimit", "Usage limit")
        var text = (limit.name ? limit.name + " · " : "") + what + " " + tr(strings, "ReachedWord", "reached") + "."
        if (limit.resetsAt > 0) {
            var reset = tr(strings, "ResetsIn", "resets in {n}", { n: countdown(new Date(limit.resetsAt * 1000).toISOString()) })
            text += " " + reset.charAt(0).toUpperCase() + reset.substring(1) + "."
        }
        return text
    }

    // Unified diff → rows for the Changes pane: { kind: hunk|add|del|ctx, old, new, text }.
    function diffLines(diff) {
        var out = []
        var lines = (diff || "").split("\n")
        var oldNo = 0, newNo = 0
        for (var i = 0; i < lines.length; ++i) {
            var line = lines[i]
            if (line.indexOf("+++") === 0 || line.indexOf("---") === 0 || line.indexOf("diff --git") === 0 || line.indexOf("index ") === 0)
                continue
            if (line.indexOf("@@") === 0) {
                var m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/)
                if (m) { oldNo = parseInt(m[1]); newNo = parseInt(m[2]) }
                out.push({ kind: "hunk", old: "", new: "", text: m && m[3] ? m[3].trim() : "" })
                continue
            }
            if (line.length === 0 && i === lines.length - 1) continue
            var c = line.charAt(0)
            if (c === "+") { out.push({ kind: "add", old: "", new: String(newNo++), text: line.substring(1) }) }
            else if (c === "-") { out.push({ kind: "del", old: String(oldNo++), new: "", text: line.substring(1) }) }
            else if (c === "\\") { continue }
            else { out.push({ kind: "ctx", old: String(oldNo++), new: String(newNo++), text: line.substring(1) }) }
        }
        return out
    }

    function diffStats(diff) {
        var add = 0, del = 0
        var lines = (diff || "").split("\n")
        for (var i = 0; i < lines.length; ++i) {
            var c = lines[i].charAt(0)
            if (c === "+" && lines[i].indexOf("+++") !== 0) add++
            else if (c === "-" && lines[i].indexOf("---") !== 0) del++
        }
        return { add: add, del: del }
    }
}
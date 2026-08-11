# Events List

A script reacts to the room by defining handler functions. This is the complete list.

> **Arg types:** `user` = [User](User.md) object, `pm` = [PM](PM.md) object, otherwise a string/number/bool.

## Return values

There are three kinds of handlers:

| Kind | Return value | Example |
|---|---|---|
| **Rewrite** | `string` replaces the message; `false`/`null`/`""` cancels it; `true`/`undefined` leaves it alone | `onTextBefore`, `onEmoteBefore`, `onPMBefore` |
| **Gate** | `false` blocks the action; anything else allows it | `onJoinCheck`, `onVroomJoinCheck`, `onScribbleCheck`, ... |
| **Fire-and-forget** | ignored | everything else |

Rewrite hooks are **chained** across scripts: the text returned by one script is passed to the next.

## Lifecycle

| Handler | Args | When | Return |
|---|---|---|---|
| `onLoad()` | — | Called once when the script loads | ignored |
| `onTimer()` | — | Called once every second (heartbeat) | ignored |

```javascript
function onLoad() {
    print("script started!");
}

function onTimer() {
    // runs once per second
}
```

## Connection

| Handler | Args | When |
|---|---|---|
| `onConnect(ip)` | `ip` | Client connected (before login) |
| `onDisconnect(ip)` | `ip` | Client disconnected |
| `onJoin(user, ip)` | `user`, `ip` | User accepted into the room |
| `onJoinCheck(user, ip)` | `user`, `ip` | **Gate**: `false` rejects the join |
| `onRejected(user, ip, reason)` | `user`, `ip`, `reason` | A join was rejected |
| `onPart(user)` | `user` | User left the room |

```javascript
function onJoinCheck(user, ip) {
    if (("" + user).indexOf("@") >= 0) {
        print("rejected " + user + " (" + ip + ")");
        return false; // block the join
    }
    return true;
}
```

## Public chat

| Handler | Args | When |
|---|---|---|
| `onPublic(user, text)` | `user`, `text` | Public message (native Astra name) |
| `onTextReceived(user, text)` | `user`, `text` | Alias sb0t of `onPublic` |
| `onTextBefore(user, text)` | `user`, `text` | **Rewrite** public text before it's sent |
| `onEmote(user, text)` | `user`, `text` | Emote received (native Astra name) |
| `onEmoteReceived(user, text)` | `user`, `text` | Alias sb0t of `onEmote` |
| `onEmoteBefore(user, text)` | `user`, `text` | **Rewrite** emote text |

```javascript
function onTextBefore(user, text) {
    var t = "" + text;
    if (t.indexOf("cancelme") >= 0) return false;   // cancel
    return t.replace(/bad/g, "***");                // rewrite
}
```

## Private messages

| Handler | Args | When |
|---|---|---|
| `onPrivate(from, to, text)` | `from`, `to`, `text` | PM received (native Astra name) |
| `onPM(from, to)` | `from`, `to` | Alias sb0t (no text argument) |
| `onPMBefore(from, to, pm)` | `from`, `to`, `pm` | **Rewrite** the PM text |
| `onBotPM(user, text)` | `user`, `text` | **Gate**: `false` blocks the bot's PM |

```javascript
function onPMBefore(from, to, pm) {
    if (pm.isScribble) return pm;
    if (pm.contains("secret")) return pm.replace("secret", "***");
    return pm;
}
```

## Commands and help

| Handler | Args | When |
|---|---|---|
| `onCommand(user, command, target, args)` | `user`, `command`, `target`, `args` | A slash command was run |
| `onHelp(user)` | `user` | `/help` was run |

`command` is the full `"cmd args"` line (sb0t parity), so read the name with `command.split(" ")`. `target` is a [User](User.md) resolved from the first token of `args`, or `null`.

```javascript
function onCommand(user, command, target, args) {
    var cmd = command.split(" ")[0];
    if (cmd === "users") {
        print("Online: " + Users.count());
    } else if (cmd === "whois") {
        if (target == null) {
            user.sendPM("usage: /whois <nick>");
        } else {
            user.sendPM(target.name + " level " + target.level);
        }
    }
}

function onHelp(user) {
    user.sendPM("/users - show the online count");
    user.sendPM("/whois <nick> - info about a user");
}
```

## Users and account events

| Handler | Args | When |
|---|---|---|
| `onNick(user, newName)` | `user`, `newName` | Nick change (also a **gate**: `false` blocks) |
| `onAvatar(user)` | `user` | **Gate**: `false` blocks avatar change |
| `onPersonalMessage(user, text)` | `user`, `text` | **Gate**: `false` blocks pmsg change |
| `onAdminLevelChanged(user)` | `user` | Admin level changed |
| `onLoginGranted(user)` | `user` | Login granted |
| `onLogout(user)` | `user` | Logout |
| `onInvalidLoginAttempt(user, ip)` | `user`, `ip` | Failed login attempt |
| `onIdled(user)` | `user` | User went idle |
| `onUnidled(user, seconds)` | `user`, `seconds` | User came back; `seconds` idle time |
| `onRegistering(user, ip)` | `user`, `ip` | **Gate**: `false` rejects registration (also an event) |
| `onRegistered(user, ip)` | `user`, `ip` | Registration completed |
| `onUnregistered(user)` | `user` | Registration removed |
| `onIgnoring(user, target)` | `user`, `target` | **Gate**: `false` blocks ignore/unignore |
| `onFileReceived(user, filename)` | `user`, `filename` | File browse received |

## Moderation

| Handler | Args | When |
|---|---|---|
| `onFlood(user)` | `user` | User flooded (punishment applied) |
| `onFloodBefore(user, msg)` | `user`, `msg` | **Gate**: `false` forgives the flood |
| `onScribbleCheck(user, isPM)` | `user`, `isPM` | **Gate**: `false` rejects the scribble |
| `onBansAutoCleared()` | — | Expired bans were cleared |
| `onProxyDetected(user, ip, reply)` | `user`, `ip`, `reply` | **Gate**: `false` rejects (default) |

```javascript
function onScribbleCheck(user, isPM) {
    if (isPM) return false; // no private scribbles
    return true;
}
```

## Vroom

| Handler | Args | When |
|---|---|---|
| `onVroomJoin(user, vroom)` | `user`, `vroom` | User entered a vroom |
| `onVroomJoinCheck(user, vroom)` | `user`, `vroom` | **Gate**: `false` blocks entering the vroom |

```javascript
function onVroomJoinCheck(user, vroom) {
    if (vroom == 9) {
        user.sendPM("vroom 9 is closed");
        return false;
    }
    return true;
}
```

## Link / leaves

| Handler | Args | When |
|---|---|---|
| `onLeafJoin(name)` | `name` | A leaf connected |
| `onLeafPart(name)` | `name` | A leaf disconnected |

## Background tasks

| Handler | Args | When |
|---|---|---|
| `__onTimerCallback(id, name)` | `id`, `name` | Internal — drives `setTimer`/`setTimeout`/`Timer` |
| `onHttpComplete(key, body, status, error)` | `key`, `body`, `status`, `error` | An [HttpRequest](HttpRequest.md) finished |

## Not wired (sb0t parity only)

These handlers exist in the engine but the server does not dispatch them yet:
`onPartBefore`, `onUserUpdate`, `onTextAfter`, `onEmoteAfter`, `onLinked`, `onUnlinked`, `onLinkError`, `onLinkedAdminDisabled`.

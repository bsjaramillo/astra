# User

The live user object (JSUser). Get one with `user(name)`, `Users.getUserByName(name)`, or as the first argument of most event handlers. In string context it behaves as the user's nick (`"" + user == user.name`).

## Properties (read-only)

```javascript
user.name            // nick (as a getter, always live)
user.orgName         // original name
user.id              // user id
user.level           // 0=regular, 1=moderator, 2=admin, 3=owner
user.vroom           // vroom number
user.externalIp      // external IP
user.localIp         // local IP
user.dns             // hostname
user.guid            // GUID (hex)
user.version         // client version
user.age             // age
user.gender          // gender ("M"/"F")
user.sex             // alias of gender
user.country         // country code
user.region          // region
user.fileCount       // shared files
user.port            // client port
user.muzzled         // bool
user.cloaked         // bool
user.registered      // bool
user.encrypted       // bool
user.owner           // bool
user.webClient       // bool
user.customClient    // bool
user.browsable       // bool
user.fastPing        // bool
user.canHTML         // bool
user.personalMessage // pmsg
user.customName      // custom nick (get/set)
user.joinTime        // join timestamp
user.captcha         // captcha info
user.idle            // bool
user.visible         // bool
user.ghost           // bool
user.localEP         // "ip:port"
user.linked          // bool (remote via link)
user.leaf            // leaf name (remote)
user.originalIp      // original IP
```

## Properties (get/set)

| Property | What it does |
|---|---|
| `customName` | Custom nick (broadcasts rename) |
| `vroom` | Move the user to another vroom |
| `level` | Change the user's level (0-3, persisted) |
| `muzzled` | Mute/unmute |
| `avatar` | Base64 avatar; set `""` or `null` to clear |

```javascript
u.customName = "CoolNick";
u.vroom = 2;
u.level = 1;
u.muzzled = true;
u.avatar = Base64.encode("...");
```

## Special properties

### `avatar`
A String OBJECT with the base64 data plus sb0t helpers: `arg`, `exists`, `save(name)`, `toScribble()`.

```javascript
var av = u.avatar;
if (av.exists) {
    av.save("backup_" + u.name + ".txt");
    var scrib = av.toScribble();
}
```

### `font`
Read-only object `{enabled, nameColor, textColor, family, ...}`. Set is a no-op (the client controls its font).

```javascript
print(u.font.nameColor + " / " + u.font.textColor);
```

### `ignores`
An ignore collection with `.count` and numeric indexing.

```javascript
for (var i = 0; i < u.ignores.count; i++) {
    print("ignored: " + u.ignores[i]);
}
```

## Methods

| Method | What it does |
|---|---|
| `ban()` | Ban the user |
| `kick()` | Kick the user (broadcasts PART) |
| `disconnect()` | Disconnect the user |
| `sendText(text)` | The user "says" `text` in public to their vroom |
| `sendEmote(text)` | The user emotes in public |
| `sendPM(text)` | PM from the bot to the user |
| `sendHTML(text)` | System/HTML PM to the user |
| `exists()` | Still online? |
| `redirect(hashlink)` | Redirect the user to another room |
| `setTopic(topic)` | Set the topic for that user |
| `nudge([sender])` | Send a cb0t nudge (default sender = bot) |
| `setUrl(addr, text)` | Send a URL only to this user; no args = clear |
| `scribble(img)` / `scribble(sender, img)` | Send a directed scribble |
| `restoreAvatar()` | Restore the avatar the client originally sent |
| `getASN()` | ASN number or `null` |

```javascript
u.sendText("this is me talking");
u.sendEmote("winks");
u.sendPM("private message");
u.kick();
```

## Example: info command

```javascript
function onCommand(user, command, target, args) {
    if (command === "whois") {
        var who = target == null ? user : target;
        who.sendPM(who.name + " → ip=" + who.externalIp +
                   " level=" + who.level + " vroom=" + who.vroom);
    }
}
```

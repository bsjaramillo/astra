# Global

Global functions available to every script.

## Messaging

### `print(text)`, `print(vroom, text)`, `print(user, text)`
Sends a message **from the bot**. With one argument it goes to the whole room; with a vroom number it goes to that vroom; with a user object it goes to that user.

```javascript
print("Hello everyone!");
print(2, "only vroom 2");
var u = user("Alice");
print(u, "private line just for Alice");
```

### `sendPublic(from, text)`
Broadcasts a public message **as if it came from `from`**.

```javascript
sendPublic("Server", "The room will close in 5 minutes.");
```

### `sendEmote(from, text)`
Broadcasts an emote **as if it came from `from`**.

```javascript
sendEmote("Server", "winks at everyone");
```

### `sendPM(from, to, text)`
Sends a private message from `from` to `to`. Returns `false` if `to` is not online.

```javascript
if (!sendPM("Server", "Alice", "psst, hello")) {
    print("Alice is offline");
}
```

### sb0t form: `sendText(user, sender, text)`, `sendEmote(user, sender, text)`, `sendPM(user, sender, text)`
The classic sb0t signature (first argument is a **user object**, not a name). These send to *that specific user*.

```javascript
var alice = user("Alice");
sendText(alice, "Server", "direct message");
sendEmote(alice, "Server", "waves at Alice");
sendPM(alice, "Server", "private hello");
```

## Users and room

```javascript
userCount();            // number of online users
userNames();            // array of online names
userExists("Alice");    // true/false
getUserIp("Alice");     // external IP
getUserLevel("Alice");  // 0..3 (0=regular, 1=moderator, 2=admin, 3=owner)
getUserVroom("Alice");  // vroom number
kickUser("Alice");      // kick from the room
getTopic();             // current room topic
setTopic("New topic");  // set room topic
```

### `user(name)`
Returns a live [User](User.md) object for that nick, or `null` if offline. Accepts any value that converts to a string.

```javascript
var u = user("Alice");
if (u != null) {
    u.sendPM("Hello from a script!");
}
```

## Script includes

### `include(name)`
Loads `<script_folder>/<name>.js` into the same script context. Returns `true` on success.

```javascript
include("helpers"); // loads myplugin/helpers.js
```

### `includeAll()`
Loads every `.js` in the script folder except the main file. Returns the number of files loaded.

```javascript
includeAll();
```

## Timers

### `setTimer(seconds, functionName)`
Calls the global function `functionName` every `seconds` seconds (repeating). Returns a timer id.

```javascript
function announce() {
    print("Reminder: be nice!");
}
var t = setTimer(60, "announce"); // every minute
```

### `setTimeout(seconds, functionName)`
Calls the global function once, after `seconds` seconds. Returns a timer id.

```javascript
function bye() {
    print("I'm going to sleep now.");
}
setTimeout(30, "bye"); // once, in 30s
```

### `clearTimer(id)`
Cancels a timer. Returns `true` if it was removed.

```javascript
clearTimer(t);
```

## Help

### `Help_addLine(command, line)`
Adds a line to the `/help` output. Lines are removed automatically when the script is unloaded.

```javascript
function onLoad() {
    Help_addLine("hola", "/hola - greet the room");
}
```

## Helpers (sb0t compatibility)

```javascript
scriptName();   // name of this script's folder
tickCount();    // Date.now() in ms
byteLength(s);  // length of s in UTF-8 bytes
stripColors(s); // removes IRC color codes
escapeUtf(s);   // encodeURIComponent(s)
clrName(obj);   // CLR-style type name, e.g. "scripting.Objects.JSUser"
```

## Hashing / encoding

```javascript
astraHash("hello");         // SHA-1 hex
astraMd5("hello");          // MD5 hex
astraBase64Encode("hello"); // base64
astraBase64Decode("aGVsbG8=");
```

## @eval

Code typed in chat prefixed with `@` is evaluated in the first loaded script (Owner only). `userobj` is preset to the sender.

```
@print("1+1 = " + (1+1))
@userobj.sendPM("hi from eval")
```

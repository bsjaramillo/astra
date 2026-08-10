# Users

Static object to query and manage online users.

## Methods

### `count()`
Number of online users.

```javascript
print("Online: " + Users.count());
```

### `getUserByName(name)`
Returns a live [User](User.md) object, or `null` if offline.

```javascript
var u = Users.getUserByName("Alice");
if (u != null) u.sendPM("hello!");
```

### `exists(name)`
Returns `true` if the user is online.

```javascript
if (Users.exists("Alice")) print("Alice is here");
```

### `names()`
Array of all online nicks.

```javascript
var nicks = Users.names();
print("Users: " + nicks.join(", "));
```

### `local(fn)` 
Returns an array of [User](User.md) objects for local users. If you pass a callback it is called with `fn(user, index)` for each one.

```javascript
Users.local(function(u, i) {
    print("user " + i + ": " + u.name + " (level " + u.level + ")");
});

var all = Users.local(); // or get the array directly
```

### `records(fn)`
History of disconnected users (JSRecord). Each record has `ban()`. Callback form: `fn(record, index)`.

```javascript
Users.records(function(r) {
    print(r.name + " was here (ip " + r.externalIp + ")");
});
```

### `banned(fn)`
Current ban list (JSBannedUser). Each entry has `unban()`. Callback form: `fn(bannedUser, index)`.

```javascript
var bans = Users.banned();
print("Active bans: " + bans.length);
for (var i = 0; i < bans.length; i++) {
    print("  " + bans[i].name + " [" + bans[i].externalIp + "]");
}

function unbanAll() {
    Users.banned(function(b) {
        if (b.externalIp === "1.2.3.4") b.unban();
    });
}
```

### `linked(fn)`
Remote users connected via [Link](Link.md). Each one has `linked = true` and a `leaf` property with the leaf name.

```javascript
Users.linked(function(u, i) {
    print(u.name + " is on leaf " + u.leaf);
});
```

## Example: "who is here"

```javascript
function onCommand(user, command, target, args) {
    if (command === "who") {
        user.sendPM("Online: " + Users.count());
        user.sendPM(Users.names().join(", "));
    }
}
```

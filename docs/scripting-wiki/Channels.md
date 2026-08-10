# Channels

In Astra, Ares "channels" map to **vrooms**. Methods let you inspect and manage vrooms.

## Properties

| Property | Type | Description |
|---|---|---|
| `available` | bool | Room-search service available |
| `enabled` | bool | Room-search enabled (same as `available`) |

## Methods

### `get(id)`
Returns a JSON object describing a vroom, or `null`.

```javascript
var ch = Channels.get(2);
if (ch != null) print("vroom 2: " + ch.name);
```

### `list()`
Array of vroom ids.

```javascript
print("Vrooms: " + Channels.list().join(", "));
```

### `create(id, name)`
Create a vroom.

```javascript
Channels.create(9, "Games");
```

### `delete(id)`
Delete a vroom.

```javascript
Channels.delete(9);
```

### `broadcast(id, from, text)`
Broadcast a public message to a vroom.

```javascript
Channels.broadcast(2, "Server", "welcome to vroom 2");
```

### `setTopic(id, topic)`
Set a vroom's topic.

```javascript
Channels.setTopic(2, "Gaming lounge");
```

### `kick(vroomId, name)`
Kick a user out of a vroom.

```javascript
Channels.kick(2, "Bob");
```

### `search(text)`
Search the room-search channel list. Returns an array of JSChannel objects with `name`, `topic`, `language`, `users`, `server`, `port`, `hashlink`, ...

```javascript
Channels.search("gaming").forEach(function(ch) {
    print(ch.name + " (" + ch.language + ") - " + ch.users + " users");
    print("  hashlink: " + ch.hashlink);
});
```

## Example: list active vrooms

```javascript
function onCommand(user, command, target, args) {
    if (command === "vrooms") {
        var ids = Channels.list();
        user.sendPM("Vrooms: " + ids.join(", "));
    }
}
```

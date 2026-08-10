# Room

Static object with information and actions for the current room.

## Properties

| Property | Type | Description |
|---|---|---|
| `name` | string | Room name |
| `topic` | string | Room topic (get/set) |
| `botName` | string | Bot's nick |
| `port` | number | Server port |
| `version` | number | Protocol version (5009) |
| `externalIp` | string | External IP reported by room-search (`""` until known) |
| `startTime` | number | Server start time |
| `hashlink` | string | Room hashlink (`arlnk://...`) |
| `customNames` | bool | Custom names allowed (get/set) |

```javascript
function onLoad() {
    print("Room: " + Room.name + " on port " + Room.port);
    print("External IP: " + Room.externalIp);
    print("Start: " + Room.startTime);
}
```

## Methods

### `setTopic(topic)` / `topic = ...`
Set the room topic.

```javascript
Room.setTopic("Welcome to my room");
Room.topic = "Welcome to my room"; // same thing
```

### `broadcast(text)`
Send a message from the bot to the whole room.

```javascript
Room.broadcast("Maintenance in 1 minute");
```

### `setUrl(addr, text)`
Sets the room's URL and announces it (web-aware). Replaces the previous URL list.

```javascript
Room.setUrl("https://example.com", "Homepage");
```

### `clearUrl()`
Clears the room's URLs.

```javascript
Room.clearUrl();
```

## Example: topic command

```javascript
function onCommand(user, command, target, args) {
    if (command === "topic") {
        if (args.length > 0) {
            Room.setTopic(args);
            print("Topic updated.");
        } else {
            user.sendPM("Current topic: " + Room.topic);
        }
    }
}
```

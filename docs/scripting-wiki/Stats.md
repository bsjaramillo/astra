# Stats

Static object with server statistics. The sb0t properties are read-only.

## Properties (sb0t, read-only)

```javascript
Stats.userCount;          // current users
Stats.peakUserCount;      // max concurrent users
Stats.joinCount;          // total joins
Stats.partCount;          // total parts
Stats.dataReceived;       // bytes received
Stats.dataSent;           // bytes sent
Stats.floodCount;         // flood events
Stats.invalidLoginCount;  // failed logins
Stats.rejectionCount;     // rejected joins
Stats.messageCount;       // public messages
Stats.pmCount;            // private messages
```

```javascript
function onCommand(user, command, target, args) {
    if (command === "stats") {
        user.sendPM("Users: " + Stats.userCount + " (peak " + Stats.peakUserCount + ")");
        user.sendPM("Messages: " + Stats.messageCount + ", PMs: " + Stats.pmCount);
    }
}
```

## Methods (Astra extras)

### `addStat(key, value)`
Increments an in-memory custom stat.

```javascript
Stats.addStat("greetings_sent", 1);
```

### `getStat(key)`
Reads a custom stat.

```javascript
print("greetings sent: " + Stats.getStat("greetings_sent"));
```

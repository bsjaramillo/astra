# Link

Static object for the server-linking feature (hub ↔ leaves).

## Properties (sb0t, read-only)

`linked`, `name`, `externalIp`, `port` (=-1 when not linked), `hashlink`.

```javascript
if (Link.linked) {
    print("Linked as " + Link.name + " (" + Link.externalIp + ":" + Link.port + ")");
    print("Hub hashlink: " + Link.hashlink);
} else {
    print("Not linked");
}
```

## Methods

### `createLink(server, port)`
Connects to a hub/leaf.

```javascript
Link.createLink("127.0.0.1", 5009);
```

### `connect(arg)`
Connects from a single string: a hashlink, `"host:port"`, or just a host (default port 5009).

```javascript
Link.connect("astrahash://127.0.0.1:5009");
Link.connect("other.example.com:5009");
```

### `disconnect([name])`
Disconnects the link. Without arguments it disconnects the current hub.

```javascript
Link.disconnect();
```

### `list()`
Array of connected link names.

```javascript
print(Link.list().join(", "));
```

### `leaves([fn])`
Array of [Leaf](Leaf.md) objects. Callback form: `fn(leaf, index)`.

```javascript
Link.leaves(function(leaf) {
    print("leaf " + leaf.name + " has " + leaf.users().length + " users");
});
```

### `leaf(name)`
Finds a [Leaf](Leaf.md) by exact name or prefix, or `null`.

```javascript
var l = Link.leaf("my-leaf");
if (l != null) l.print("hello from the hub");
```

### `findHub(name)`, `findLeaf(name)`, `findUser(name)`
Look up by name across the link network.

```javascript
var u = Link.findUser("Alice");
if (u != null) print("Alice is on " + u.leaf);
```

### `getUserList()`
Raw list of linked users.

```javascript
print(Link.getUserList());
```

### `kickHub(name)`
Disconnects a hub/leaf by name.

```javascript
Link.kickHub("bad-leaf");
```

## Example: relay a message to all leaves

```javascript
function onCommand(user, command, target, args) {
    if (command === "leafmsg") {
        var ok = false;
        Link.leaves(function(leaf) {
            leaf.print(args);
            ok = true;
        });
        user.sendPM(ok ? "Sent to leaves." : "No leaves connected.");
    }
}
```

# Leaf

A leaf server connected via [Link](Link.md). Real leaf objects come from `Link.leaves()` / `Link.leaf(name)`. `new Leaf()` gives an empty object (compatibility).

```javascript
var leaves = Link.leaves();
var l = leaves[0];
```

## Properties

```javascript
leaf.ident;        // numeric ident assigned by the hub
leaf.name;         // leaf name
leaf.externalIp;   // external IP
leaf.port;         // port
leaf.hashlink;     // hashlink
```

## Methods

| Method | What it does |
|---|---|
| `print(text)` or `print(vroom, text)` | System line on the leaf (whole room or a vroom) |
| `printAdmins(text)` or `printAdmins(level, text)` | To admins with level > N (default 1 = Moderator+) |
| `users([fn])` | Users attributed to this leaf (callback `fn(user, i)`) |
| `user(name)` | Find a user on this leaf (exact or prefix) |
| `sendText(sender, text)` | The leaf broadcasts a public message as `sender` |
| `sendEmote(sender, text)` | The leaf broadcasts an emote as `sender` |
| `scribble(img)` or `scribble(sender, img)` | Send a scribble to the leaf |

## Example: announce to all leaves

```javascript
function onCommand(user, command, target, args) {
    if (command === "announceall") {
        Link.leaves(function(leaf) {
            leaf.print(args);
        });
    }
}
```

## Example: targeted message

```javascript
var l = Link.leaf("backup-leaf");
if (l != null) {
    l.sendText("Server", "This is a hub-wide public message!");
}
```

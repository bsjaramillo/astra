# Entities

Static object for the UDP room-search entities (server nodes) and HTML escaping.

## Methods

### `list()`
Array of room-search nodes `{name, port, users}`.

```javascript
Entities.list().forEach(function(e) {
    print(e.name + ":" + e.port + " (" + e.users + " users)");
});
```

### `encode(s)`
HTML-escapes a string (`& < > " '`).

```javascript
var safe = Entities.encode("<b>hi & bye</b>");
print(safe); // "&lt;b&gt;hi &amp; bye&lt;/b&gt;"
```

### `decode(s)`
Un-escapes HTML entities back to text.

```javascript
var plain = Entities.decode("&lt;b&gt;hi&lt;/b&gt;");
print(plain); // "<b>hi</b>"
```

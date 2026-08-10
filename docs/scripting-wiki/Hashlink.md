# Hashlink

Static object to create and parse hashlinks (the `astrahash://` URLs used to join rooms).

## Methods

### `create(server, port)`
Returns a hashlink string for a server:port.

```javascript
var hl = Hashlink.create("127.0.0.1", 5009);
print(hl); // "astrahash://127.0.0.1:5009"
```

### `parse(url)`
Parses a hashlink. Returns a JSON string `{"server":"...","port":...}`, or `null` if it can't be parsed.

```javascript
var info = Hashlink.parse("astrahash://example.com:5009");
if (info != null) {
    var o = JSON.parse(info);
    print(o.server + ":" + o.port);
}
```

### Aliases
`encode` = `create`, `decode` = `parse`.

## Example: redirect a user

```javascript
function redirectUser(user) {
    var hl = Hashlink.create("other.example.com", 5009);
    user.redirect(hl);
}
```

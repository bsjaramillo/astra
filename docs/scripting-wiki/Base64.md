# Base64

Static object for Base64 encoding.

## Methods

### `encode(s)`
Base64-encodes a string.

```javascript
var b64 = Base64.encode("hello world");
print(b64); // "aGVsbG8gd29ybGQ="
```

### `decode(s)`
Base64-decodes a string. Returns `null` if the input is invalid.

```javascript
var plain = Base64.decode("aGVsbG8gd29ybGQ=");
print(plain); // "hello world"
```

## Example: store an avatar encoded

```javascript
function onAvatar(user) {
    var b64 = user.avatar; // already base64
    File.save("avatars_" + user.name + ".txt", b64);
}
```

# Crypto

Static object for hashing. There are two flavors: plain-hex helpers and sb0t-style `CryptoResult` objects.

## Methods

### `hashSHA1(s)` / `sha1(s)`
SHA-1 hash as a hex string.

```javascript
print(Crypto.hashSHA1("hello")); // "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
```

### `hashMD5(s)` / `md5(s)`
MD5 hash as a hex string.

```javascript
print(Crypto.hashMD5("hello")); // "5d41402abc4b2a76b9719d911017c592"
```

### `sha1hash(s)` / `md5hash(s)`
Returns a **CryptoResult** object with `toHex()`, `toBase64()`, `toArray()` and `toString()`.

```javascript
var r = Crypto.sha1hash("hello");
print(r.toHex());       // hex string
print(r.toBase64());    // base64
print(r.toArray());     // byte array

var m = Crypto.md5hash("hello");
print(m.toHex());       // "5d41402abc4b2a76b9719d911017c592"
```

## Example: verify a stored hash

```javascript
function onCommand(user, command, target, args) {
    if (command === "pw") {
        var stored = "5d41402abc4b2a76b9719d911017c592";
        if (Crypto.hashMD5(args) === stored) {
            user.sendPM("Password matches!");
        } else {
            user.sendPM("Wrong password.");
        }
    }
}
```

# Zip

Static object for compression. Data is compressed into a base64-encoded zip.

## Methods

### `compress(s)`
Compresses a string. Returns the base64 of a zip archive.

```javascript
var zip = Zip.compress("some text to compress");
print("compressed: " + zip);
```

### `uncompress(s)` / `decompress(s)`
Decompresses base64 zip data back to text.

```javascript
var text = Zip.decompress(zip);
print(text); // "some text to compress"
```

## Example: store a compressed log

```javascript
File.save("log.zip.b64", Zip.compress("line1\r\nline2"));
```

# File

Static object for reading/writing files. Paths are resolved inside the **script's own `data/` subfolder** (reads also fall back to the script folder root).

```
myplugin/
└── data/          ← File.save("x.txt", ...) writes here
    └── x.txt
```

## Methods

### `exists(name)`
Returns `true` if the file exists.

```javascript
if (File.exists("count.txt")) print("count file exists");
```

### `load(name)` / `read(name)`
Reads the file contents as a string.

```javascript
var n = File.load("count.txt") || "0";
```

### `save(name, text)` / `write(name, text)`
Writes `text` to the file (overwrites).

```javascript
File.save("count.txt", "42");
```

### `append(name, text)`
Appends `text` to the file.

```javascript
File.append("log.txt", "a line");
```

### `appendLine(name, text)`
Appends `text` followed by `\r\n`.

```javascript
File.appendLine("log.txt", "user joined");
```

### `kill(name)` / `delete(name)`
Deletes the file.

```javascript
File.kill("temp.txt");
```

### `size(name)`
File size in bytes.

```javascript
print("size: " + File.size("log.txt"));
```

### `creationTime(name)`
File creation time as a UNIX timestamp (seconds).

```javascript
print("created: " + File.creationTime("log.txt"));
```

## Example: persistent counter

```javascript
function onJoin(user) {
    var n = parseInt(File.load("count.txt") || "0", 10) + 1;
    File.save("count.txt", "" + n);
    print("Join counter is now " + n);
}
```

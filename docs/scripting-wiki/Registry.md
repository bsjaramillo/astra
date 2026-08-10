# Registry

Static object for a persistent key/value store. Values survive restarts and are stored per-script in `<script>/registry.json`. Keys live under a virtual `HKLM\Software\Astra\` hive (sb0t compatibility).

## Methods

### `createKey(name)`
Creates a key in the virtual hive.

```javascript
Registry.createKey("plugins");
```

### `deleteKey(name)`
Deletes a key.

```javascript
Registry.deleteKey("plugins");
```

### `setValue(name, value)`
Stores a value.

```javascript
Registry.setValue("greet_message", "Welcome!");
```

### `getValue(name)`
Reads a value (`null` if missing).

```javascript
var msg = Registry.getValue("greet_message") || "Welcome!";
```

### `exists(name)`
Returns `true` if the value exists.

```javascript
if (Registry.exists("greet_message")) print("greeting configured");
```

### `getKeys()`
Array of all keys.

```javascript
print(Registry.getKeys().join(", "));
```

### `deleteValue(name)`
Removes a value.

```javascript
Registry.deleteValue("greet_message");
```

### `clear()`
Clears the whole registry.

```javascript
Registry.clear();
```

## Example: load a setting once

```javascript
function onLoad() {
    if (!Registry.exists("greet_message")) {
        Registry.setValue("greet_message", "Welcome!");
    }
    GREETING = Registry.getValue("greet_message");
}
```

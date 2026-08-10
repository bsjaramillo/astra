# Script

Static object for including other files from the script folder.

## Methods

### `include(name)`
Loads `<script_folder>/<name>.js` into the same context. Alias of the global `include()`.

```javascript
Script.include("helpers");
```

See also [Global](Global.md#script-includes) for `include()` / `includeAll()`.

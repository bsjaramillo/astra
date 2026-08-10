# Scribble

A scribble (drawn image) object, created from a base64 string.

```javascript
var s = new Scribble(base64string);
```

## Properties

| Property | What it does |
|---|---|
| `src` | Base64 data (get/set) |
| `size` | Image size in bytes (-1 if empty) |
| `oncomplete` | Callback used with `download()` |

## Methods

| Method | What it does |
|---|---|
| `save(path)` | Save to `<script>/data/<path>` |
| `load(path)` | Load from a file into the object |
| `download(url)` | Fetch the image from a URL (async) |

## Example: relay a scribble to a user

```javascript
function onCommand(user, command, target, args) {
    if (command === "scrib") {
        var img = new Scribble(Base64.encode("...")); // a drawn image
        target.scribble(img);
    }
}
```

## Converting to/from avatar

```javascript
var av = new Avatar(b64);
var scrib = av.toScribble();        // avatar → scribble
var av2 = scrib.toAvatar();         // scribble → avatar
```

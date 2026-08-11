# Scribble

A scribble (drawn image) object, created from a base64 string or downloaded from a URL.

```javascript
var s = new Scribble(base64string);
```

## Properties

| Property | What it does |
|---|---|
| `src` | Base64 data (get/set). Also accepts `http://`/`https://` URLs for `download()`. |
| `size` | Image size in bytes (-1 if empty) |
| `oncomplete` | Callback used with `download()` |

## Methods

| Method | What it does |
|---|---|
| `save(path)` | Save to `<script>/data/<path>` |
| `load(path)` | Load from a file into the object |
| `download(url?)` | Fetch the image from a URL (async). If no URL is given, uses `scribble.src`. |

## Download and send to all users

```javascript
function sendScribble(url) {
  var scribble = new Scribble();
  scribble.oncomplete = function (e) {
    if (!e || e.__id < 0) return;
    Users.local(function (u) {
      u.scribble(e);
    });
  };
  scribble.download(url);
}
```

You can also set `scribble.src = url` and call `scribble.download()` without arguments — both patterns work.

## `scribbleGif` (web clients only)

Sends the scribble URL directly as a `SCRIBBLE_GIF` packet — the sb0t `WebOutbound.ScribbleGifTo` wire format. The scribble's `src` must be an `http://` or `https://` URL. Only supported for web clients.

Wire format: `SCRIBBLE_GIF:senderLen,urlLen:sender+url`

```javascript
var sc = new Scribble();
sc.src = "https://example.com/drawing.png";

// Envía la URL como SCRIBBLE_GIF a un web client específico
Users.byName("WebUser").scribbleGif(sc);

// O con sender personalizado:
Users.byName("WebUser").scribbleGif("Anon", sc);
```

If the scribble has no URL (base64 only), use `scribble()` instead — `scribbleGif` requires a URL.

## Converting to/from avatar

```javascript
var av = new Avatar(b64);
var scrib = av.toScribble();        // avatar → scribble
var av2 = scrib.toAvatar();         // scribble → avatar
```

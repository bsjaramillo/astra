# Avatar

An avatar image object (sb0t JSAvatar), created from a base64 string.

```javascript
var av = new Avatar(base64string);
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
| `setForUser(name)` | Set this avatar on a user |
| `download(url)` | Fetch the image from a URL (async; calls `oncomplete`) |

## Example: backup every avatar

```javascript
function onJoin(user) {
    var av = user.avatar; // AvatarImage (string object)
    if (av.exists) {
        av.save("avatars/" + user.name + ".txt");
    }
}
```

## Example: force an avatar

```javascript
var av = new Avatar(Base64.encode("my image bytes"));
av.setForUser("Alice");
```

## Example: download an avatar from the web

```javascript
var av = new Avatar();
av.oncomplete = function(a) {
    if (a.size > 0) a.setForUser("Alice");
};
av.download("https://example.com/avatar.png");
```

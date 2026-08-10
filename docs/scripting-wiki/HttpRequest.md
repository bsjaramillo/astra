# HttpRequest

Async HTTP request object (sb0t JSHttpRequest). Requests run in a background thread; the result arrives through `oncomplete`.

```javascript
var r = new HttpRequest();
```

## Properties

| Property | What it does |
|---|---|
| `method` | `"GET"` or `"POST"` (default `"GET"`) |
| `src` | Full URL (or use `host` + `params`) |
| `host` | Host, if not using `src` |
| `params` | For GET: appended as query string; for POST: the body |
| `userAgent` | User-Agent header |
| `accept` | Accept header |
| `utf` | UTF-8 mode (bool) |
| `oncomplete` | `function(result, status, error)` |
| `response` | Set after completion (body text) |
| `status` | Set after completion (HTTP status) |
| `error` | Set after completion |

## Methods

| Method | What it does |
|---|---|
| `header(name, value)` | Add a custom header (chainable) |
| `download([arg])` | Start the request (returns bool) |

The result passed to `oncomplete` is a String OBJECT of the body with `.page`, `.arg`, `.status` and `.error`.

## Example: GET

```javascript
var r = new HttpRequest();
r.src = "https://api.example.com/hello";
r.oncomplete = function(result, status, error) {
    if (status === 200) {
        print("body: " + result.page);
    } else {
        print("error: " + error + " (status " + status + ")");
    }
};
r.download("mykey");
```

## Example: POST with headers

```javascript
var r = new HttpRequest();
r.method = "POST";
r.src = "https://api.example.com/login";
r.params = "user=alice&pass=secret";
r.header("Content-Type", "application/x-www-form-urlencoded");
r.oncomplete = function(result, status) {
    print("login returned " + status);
};
r.download();
```

## Example: onHttpComplete handler

The result also arrives at the global handler:

```javascript
function onHttpComplete(key, body, status, error) {
    if (key === "mykey") {
        print("done: " + status + " " + body);
    }
}
```

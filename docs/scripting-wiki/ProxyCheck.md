# ProxyCheck

Proxy/VPN detection via [proxycheck.io](https://proxycheck.io) (sb0t JSProxyCheck).

```javascript
var p = new ProxyCheck("your-api-key"); // apiKey optional
```

## Properties

| Property | What it does |
|---|---|
| `apiKey` | proxycheck.io API key (optional) |
| `includeVPN` | Also flag VPNs (bool, default true) |
| `useTLS` | Use HTTPS (bool, default false) |

## Methods

### `query(userOrIp, callback)`
Checks an IP (or a [User](User.md) object) and calls `callback(result, status, error)`.

The result has `{error, proxy, type, provider}` where `proxy` is a bool.

```javascript
var p = new ProxyCheck();
p.query("1.2.3.4", function(result) {
    if (result.proxy) {
        print("proxy detected: " + result.type + " (" + result.provider + ")");
    } else {
        print("no proxy: " + (result.error || "ok"));
    }
});
```

## Example: flag proxy users on join

```javascript
function onJoin(user) {
    var p = new ProxyCheck();
    p.query(user, function(result) {
        if (result.proxy) {
            user.sendPM("Please disable your VPN/proxy.");
        }
    });
}
```

# Timer

A repeating timer object (sb0t JSTimer). Backed by `setTimer`/`clearTimer`.

## Properties

```javascript
timer.interval;    // milliseconds between ticks (default 1000)
timer.oncomplete;  // function called on each tick
```

## Methods

| Method | What it does |
|---|---|
| `start()` | Start ticking (chainable) |
| `stop()` | Stop the timer (chainable) |

## Example

```javascript
var t = new Timer();
t.interval = 5000; // every 5 seconds
t.oncomplete = function() {
    print("5 seconds passed");
};
t.start();
```

Stop it later:

```javascript
t.stop();
```

Note: a plain `onTimer()` handler in your script is called once per second as a heartbeat (sb0t parity) — independent of `setTimer`.

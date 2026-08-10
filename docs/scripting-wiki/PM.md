# PM

Private message object (JSPM). In handlers it behaves like a string but also has sb0t helpers. Create one with `new PM(text)`.

## Methods

```javascript
pm.contains("word");   // true if the text contains "word"
pm.remove("bad");      // remove all occurrences (returns a new PM)
pm.replace("a", "b");  // replace all occurrences (returns a new PM)
pm.isScribble          // true if the message is a scribble (property)
pm.toString();         // the text
```

## Example: sanitize an incoming PM

```javascript
function onPMBefore(from, to, pm) {
    // pm has all string methods PLUS the JSPM helpers
    if (pm.isScribble) return pm; // don't touch scribbles
    if (pm.contains("badword")) {
        return pm.replace("badword", "***");
    }
    return pm;
}
```

## Example: block empty messages

```javascript
function onPMBefore(from, to, pm) {
    if (("" + pm).trim().length === 0) return false; // cancel
    return pm;
}
```

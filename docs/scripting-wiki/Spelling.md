# Spelling

Static object for spell checking.

## Methods

### `check(text)`
Checks a whole text. Returns `null` if everything is OK, or the text with misspelled words wrapped in `[- ... -]`.

```javascript
var res = Spelling.check("hello worlld");
if (res != null) {
    print("misspellings found: " + res);
    // "hello [-worlld-]"
}
```

### `suggest(word)`
Returns an array of suggested corrections.

```javascript
var suggestions = Spelling.suggest("worlld");
print(suggestions); // e.g. ["world", "word"]
```

### `confirm(word)`
Returns `true` if the word is in the dictionary (accepted).

```javascript
if (Spelling.confirm("hello")) print("'hello' is a word");
```

## Example: gentle spell-check reminder

```javascript
function onTextReceived(user, text) {
    var res = Spelling.check(text);
    if (res != null) {
        user.sendPM("Did you mean: " + res);
    }
}
```

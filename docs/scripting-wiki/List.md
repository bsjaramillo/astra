# List

A typed collection, like sb0t's JSList.

```javascript
var list = new List();
```

## Properties

```javascript
list.count;   // number of items
list.length;  // same as count
```

## Methods

| Method | What it does |
|---|---|
| `add(x)` | Add an item (chainable) |
| `addRange(arr)` | Add all items from an array |
| `insert(i, x)` | Insert at index |
| `insertRange(i, arr)` | Insert an array at index |
| `remove(x)` | Remove first match, returns bool |
| `removeAt(i)` | Remove at index |
| `removeRange(i, n)` | Remove n items from index |
| `removeAll(f)` | Remove items matching `f` |
| `get(i)` | Item at index |
| `getRange(i, n)` | Slice of n items |
| `indexOf(x)` / `lastIndexOf(x)` | Index of item |
| `find(f)` | First item matching `f` |
| `findAll(f)` | All items matching `f` |
| `findIndex(f)` / `findLastIndex(f)` | Index of first/last match |
| `clear()` | Empty the list |
| `reverse()` | Reverse (chainable) |
| `sort(f)` | Sort with comparator (chainable) |
| `join(sep)` | Join items into a string |

## Example

```javascript
var list = new List();
list.add("alpha").add("beta").add("gamma");

print(list.count);               // 3
print(list.get(1));              // "beta"
print(list.join(", "));          // "alpha, beta, gamma"

list.remove("beta");
print(list.count);               // 2

var found = list.find(function(s) { return s.indexOf("a") >= 0; });
print(found);                    // "alpha"
```

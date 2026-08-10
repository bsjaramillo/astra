# Sql

A SQLite database owned by the script. Databases live in `<script>/sql/`.

```javascript
var db = new Sql();
```

## Properties

```javascript
db.connected;  // bool
db.canRead;    // bool
db.lastError;  // string
```

## Methods

| Method | What it does |
|---|---|
| `open(file)` | Open `<script>/sql/<file>` (bool) |
| `query(q)` | Run a [Query](Query.md); returns the number of rows |
| `value(col)` | Read column `col` of the current result set |
| `close()` | Close the database |

## Example: a simple key/value store

```javascript
function onLoad() {
    var db = new Sql();
    if (!db.open("mydata.db")) {
        log("could not open sql: " + db.lastError);
        return;
    }
    db.query(new Query("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT)"));
    db.close();
}

function onCommand(user, command, target, args) {
    var db = new Sql();
    if (!db.open("mydata.db")) {
        user.sendPM("db error: " + db.lastError);
        return;
    }
    if (command === "put") {
        var parts = args.split(" ");
        db.query(new Query("INSERT OR REPLACE INTO kv VALUES('" + parts[0] + "','" + parts[1] + "')"));
    } else if (command === "get") {
        db.query(new Query("SELECT v FROM kv WHERE k='" + args + "'"));
        user.sendPM("value: " + (db.value("v") || "not found"));
    }
    db.close();
}
```

Note: always check `db.connected` after `open()` — scripts that silently ignore DB errors fail in confusing ways.

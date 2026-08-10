# Query

A SQL query object passed to `Sql.query()`. Build it with `new Query(sql, ...params)`.

```javascript
var q = new Query("SELECT * FROM users WHERE level >= ?", 2);
```

In practice queries are written by interpolating values directly (see [Sql](Sql.md)):

```javascript
db.query(new Query("CREATE TABLE IF NOT EXISTS kv (k TEXT, v TEXT)"));
db.query(new Query("INSERT INTO kv VALUES('" + key + "','" + val + "')"));
db.query(new Query("SELECT v FROM kv WHERE k='" + key + "'"));
```

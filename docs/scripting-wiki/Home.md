# Astra Scripting

Scripts extend the server with JavaScript (via the `boa_engine` JS engine). The API is compatible with the classic **sb0t** scripting API, so most sb0t scripts work without changes.

## How scripts work

- Scripts are plain `.js` files.
- They define **event handler functions** (`onLoad`, `onJoin`, `onCommand`, ...). The server calls them when things happen.
- They can use **global functions** and **static objects** (`Room`, `Users`, `Base64`, ...) to act on the room.

## Where scripts live

Scripts are loaded from `<data_dir>/scripts/`. With the default config this is `./data/scripts/` (change it with `--data-dir <dir>`).

```
data/scripts/
├── greet.js       # a single flat script
└── myplugin/      # or a folder = one script
    ├── myplugin.js  # main file
    ├── helpers.js   # loaded with include("helpers")
    └── data/        # files saved with File.save() go here
```

Each folder is **one script**. The main file is resolved in this order:
`<folder>.js` → `main.js` → `index.js` → first `.js` that declares a handler → first alphabetical `.js`.

## A minimal script

Save this as `data/scripts/greet.js`:

```javascript
function onLoad() {
    print("greet.js loaded!");
}

function onJoin(user) {
    print("Welcome " + user.name + "!");
    user.sendPM("Welcome to the room. Type /help for commands.");
}

function onCommand(user, command, target, args) {
    var cmd = command.split(" ")[0]; // command is the full "cmd args" line (sb0t)
    if (cmd === "hola") {
        print("Hello " + user.name + "!");
    }
}
```

## Script management commands (owner)

| Command | What it does |
|---|---|
| `/listscripts` | List loaded scripts |
| `/loadscript <name>` | Load a script from disk |
| `/killscript <name>` | Unload a running script |
| `/livescripts` | Search GitHub for community scripts |
| `/downloadscript <owner/repo>` | Download and load a script from GitHub |
| `/errors on` / `/errors off` | Receive a PM whenever a script throws an error |

Scripts in `data/scripts/` are loaded automatically at startup.

## @eval from chat

If your nick is the **Owner** you can evaluate JavaScript directly in chat. The global `userobj` is preset to you:

```
@print("2+2 = " + (2+2))
@userobj.sendPM("hi to myself")
```

## Wiki pages

### Global functions
- [Global](Global.md) — `print`, `sendPublic`, `sendPM`, `user()`, timers, helpers

### Static objects (sb0t-compatible)
- [Room](Room.md)
- [Users](Users.md)
- [Channels](Channels.md)
- [Base64](Base64.md)
- [Zip](Zip.md)
- [Hashlink](Hashlink.md)
- [Entities](Entities.md)
- [Crypto](Crypto.md)
- [File](File.md)
- [Registry](Registry.md)
- [Spelling](Spelling.md)
- [Stats](Stats.md)
- [Link](Link.md)
- [Script](Script.md)

### Objects
- [User](User.md) — the user object (JSUser)
- [PM](PM.md)
- [List](List.md)
- [Timer](Timer.md)
- [Avatar](Avatar.md)
- [Scribble](Scribble.md)
- [HttpRequest](HttpRequest.md)
- [ProxyCheck](ProxyCheck.md)
- [Sql](Sql.md)
- [Query](Query.md)
- [XmlParser](XmlParser.md)
- [Leaf](Leaf.md)

### Events
- [Events List](Events.md) — every handler your script can define

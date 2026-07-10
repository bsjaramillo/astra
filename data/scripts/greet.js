// greet.js — Script de ejemplo para Astra
//
// Handlers disponibles (compatibles con sb0t):
//   onLoad()            - al cargar
//   onUserJoin(name, ip) - cuando un user se une
//   onUserPart(name)    - cuando un user se va
//   onPublic(from, text) - mensaje público
//   onEmote(from, text)  - emote
//   onPrivate(from, to, text) - PM
//   onCommand(from, cmd, args) - comando slash

function onLoad() {
    print("greet.js cargado!");
}

// Saluda a cada usuario que se une.
function onUserJoin(name, ip) {
    print("[" + name + "] se unio desde " + ip);
    // El user es anonimo por default → PM de bienvenida
    sendPM("Bot", name, "Bienvenido a la sala! Escribe /help para ver los comandos.");
}

// Se despide.
function onUserPart(name) {
    print("[" + name + "] se fue");
}

// Log de mensajes públicos.
function onPublic(from, text) {
    print("[public] " + from + ": " + text);
}

// Comandos slash custom.
function onCommand(from, command, args) {
    if (command === "hola") {
        sendPublic("Bot", "Hola " + from + "!");
    } else if (command === "usuarios") {
        sendPublic("Bot", "Somos " + userCount() + " usuarios");
    } else if (command === "quien") {
        var name = args.trim();
        if (name.length === 0) {
            sendPublic("Bot", "uso: /quien <nick>");
            return;
        }
        if (!userExists(name)) {
            sendPublic("Bot", name + " no esta conectado");
            return;
        }
        var ip = getUserIp(name);
        var level = getUserLevel(name);
        sendPublic("Bot", name + " → ip=" + ip + " nivel=" + level);
    } else if (command === "topico") {
        if (args.length > 0) {
            setTopic(args);
            sendPublic("Bot", "Topic actualizado.");
        } else {
            sendPublic("Bot", "Topic actual: " + getTopic());
        }
    } else if (command === "hash") {
        if (args.length === 0) {
            sendPublic("Bot", "uso: /hash <texto>");
            return;
        }
        sendPM("Bot", from,
            "SHA1: " + astraHash(args) + "\n" +
            "MD5:  " + astraMd5(args)
        );
    }
}

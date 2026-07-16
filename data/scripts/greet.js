// greet.js — Script de ejemplo para Astra
//
// Handlers (nombres y firmas de sb0t):
//   onLoad()                              - al cargar el script
//   onJoin(user)                          - un usuario entró
//   onJoinCheck(user, ip)                 - return false RECHAZA el join
//   onPart(user)                          - un usuario se fue
//   onTextReceived(user, text)            - mensaje público (post-broadcast)
//   onTextBefore(user, text)              - return string REESCRIBE el texto,
//                                           false/null/"" lo CANCELA
//   onEmoteReceived(user, text)           - emote
//   onPM(user, target)                    - PM entre usuarios
//   onCommand(user, command, target, args)- comando (#cmd o /cmd).
//                                           `target` = usuario del 1er token
//                                           de args (objeto user) o null.
//   onHelp(user)                          - el usuario pidió ayuda
//
// El primer argumento es un OBJETO user: tiene .name, .id, .level, .vroom,
// .externalIp, etc., métodos .ban()/.kick()/.sendPM()/.sendText(), y setters
// (.vroom = 2, .customName = "X", .level = 50, .muzzled = true).
// En contexto string se comporta como su nombre ("" + user == user.name).

function onLoad() {
    print("greet.js cargado!");
}

// Saluda a cada usuario que entra.
function onJoin(user) {
    print("[" + user.name + "] entró desde " + user.externalIp);
    user.sendPM("Bienvenido a la sala! Escribe #help para ver los comandos.");
}

function onPart(user) {
    print("[" + user.name + "] se fue");
}

// Log de mensajes públicos.
function onTextReceived(user, text) {
    print("[public] " + user.name + ": " + text);
}

// Líneas propias en el #help (se muestran junto a las del server).
function onHelp(user) {
    user.sendPM("/hola - saludo del bot");
    user.sendPM("/quien <nick> - info de un usuario");
}

// Comandos custom. OJO: la firma tiene 4 argumentos (paridad sb0t).
function onCommand(user, command, target, args) {
    if (command === "hola") {
        print("Hola " + user.name + "!");
    } else if (command === "usuarios") {
        print("Somos " + Users.count() + " usuarios");
    } else if (command === "quien") {
        // `target` ya viene resuelto si el 1er token de args es un usuario.
        if (target == null) {
            user.sendPM("uso: /quien <nick de alguien conectado>");
            return;
        }
        user.sendPM(target.name + " → ip=" + target.externalIp +
                    " nivel=" + target.level + " vroom=" + target.vroom);
    } else if (command === "topico") {
        if (args.length > 0) {
            Room.setTopic(args);
            print("Topic actualizado.");
        } else {
            user.sendPM("Topic actual: " + Room.topic);
        }
    } else if (command === "hash") {
        if (args.length === 0) {
            user.sendPM("uso: /hash <texto>");
            return;
        }
        user.sendPM("SHA1: " + Crypto.hashSHA1(args));
        user.sendPM("MD5:  " + Crypto.hashMD5(args));
    }
}

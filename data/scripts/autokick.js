// autokick.js — Auto-kick a users con nick sospechoso
// Demuestra: kickUser, userCount, userNames, sendPublic

var MAX_NICK_LENGTH = 25;       // nicks más largos son spam
var BAD_CHARS = ["@", "#", "$"]; // chars típicos de bots

function onUserJoin(name, ip) {
    // Verificar longitud
    if (name.length > MAX_NICK_LENGTH) {
        print("auto-kick " + name + " (nick muy largo)");
        sendPublic("Bot", name + " fue kickeado por nick sospechoso");
        kickUser(name);
        return;
    }

    // Verificar chars sospechosos
    for (var i = 0; i < BAD_CHARS.length; i++) {
        if (name.indexOf(BAD_CHARS[i]) !== -1) {
            print("auto-kick " + name + " (char sospechoso)");
            kickUser(name);
            return;
        }
    }
}

function onLoad() {
    print("autokick.js activo: max_len=" + MAX_NICK_LENGTH + ", bad_chars=" + BAD_CHARS);
}

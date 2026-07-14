// autokick.js — Rechaza nicks sospechosos ANTES de que entren.
//
// Demuestra: onJoinCheck (gate cancelable: return false rechaza el join),
// user.kick(), Users.count().

var MAX_NICK_LENGTH = 25;        // nicks más largos son spam
var BAD_CHARS = ["@", "#", "$"]; // chars típicos de bots

function onLoad() {
    print("autokick.js activo: max_len=" + MAX_NICK_LENGTH + ", bad_chars=" + BAD_CHARS);
}

// Gate de entrada: retornar false RECHAZA el login (el usuario nunca llega
// a aparecer en la sala). Paridad sb0t `Joining`.
function onJoinCheck(user, ip) {
    var name = "" + user;

    if (name.length > MAX_NICK_LENGTH) {
        print("rechazado " + name + " (nick muy largo) desde " + ip);
        return false;
    }

    for (var i = 0; i < BAD_CHARS.length; i++) {
        if (name.indexOf(BAD_CHARS[i]) !== -1) {
            print("rechazado " + name + " (char sospechoso) desde " + ip);
            return false;
        }
    }

    return true;
}

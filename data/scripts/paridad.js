// paridad.js — Script de VERIFICACIÓN MANUAL de la auditoría de scripting.
//
// Cargalo con `#loadscript paridad` y probá cada punto desde el chat. Cada
// bloque corresponde a un hallazgo de docs/AUDITORIA-SCRIPTING.md.
//
// Cuando termines de verificar, descargalo con `#killscript paridad`.

function onLoad() {
    print("paridad.js cargado — probá: 'feo', '#pruebas', '@print(1+1)'");
}

// ---------------------------------------------------------------- S-A
// onTextBefore REESCRIBE el texto (antes solo podía cancelar).
// Probá escribiendo "que feo dia" en público → debe salir "que *** dia".
// Escribí "cancelame" → el mensaje no debe aparecer.
function onTextBefore(user, text) {
    var t = "" + text;
    if (t.indexOf("cancelame") >= 0) return false;   // cancela
    return t.replace(/feo/g, "***");                 // reescribe
}

// Lo mismo para emotes: probá "/me se siente feo".
function onEmoteBefore(user, text) {
    return ("" + text).replace(/feo/g, "***");
}

// ---------------------------------------------------------------- S-B
// onVroomJoinCheck rechaza cambios de vroom. Probá "#vroom 9" → no pasa nada.
function onVroomJoinCheck(user, vroom) {
    if (("" + vroom) === "9") {
        user.sendPM("el vroom 9 esta cerrado (onVroomJoinCheck)");
        return false;
    }
    return true;
}

// ---------------------------------------------------------------- S-F, S-C, S-E
// onCommand ahora recibe `target` (objeto user o null) como 3er argumento.
function onCommand(user, command, target, args) {
    if (command !== "pruebas") return;

    var sub = ("" + args).split(" ")[0];

    // S-C: el usuario "dice" el texto en público (NO es un PM del bot).
    //   #pruebas hablar <nick>
    if (sub === "hablar") {
        if (target == null) { user.sendPM("uso: #pruebas hablar <nick conectado>"); return; }
        target.sendText("esto lo digo yo, no el bot (S-C)");
        target.sendEmote("emotea sin querer (S-C)");
        return;
    }

    // S-D: kick con PART broadcast — el usuario debe DESAPARECER de la lista
    // de todos los clientes, no quedar como fantasma.
    //   #pruebas kick <nick>
    if (sub === "kick") {
        if (target == null) { user.sendPM("uso: #pruebas kick <nick conectado>"); return; }
        user.sendPM("kickeando a " + target.name + " — revisá que desaparezca de la userlist");
        target.kick();
        return;
    }

    // S-E: setters writable + props nuevas.
    //   #pruebas setters <nick>
    if (sub === "setters") {
        if (target == null) { user.sendPM("uso: #pruebas setters <nick conectado>"); return; }
        target.customName = "ProbandoX";   // debe verse el custom name
        target.vroom = 2;                  // debe moverse de vroom
        target.muzzled = true;             // debe quedar muzzleado
        user.sendPM(target.name + ": customName=" + target.customName +
                    " vroom=" + target.vroom + " muzzled=" + target.muzzled +
                    " idle=" + target.idle + " visible=" + target.visible +
                    " localEP=" + target.localEP);
        return;
    }

    // S-F: target resuelto (o null si el 1er token no es un usuario).
    if (sub === "target") {
        user.sendPM("target = " + (target == null ? "null" : target.name + " (id " + target.id + ")"));
        return;
    }

    // S-H: Users.banned() real, con unban().
    if (sub === "bans") {
        var b = Users.banned();
        user.sendPM("bans activos: " + b.length);
        for (var i = 0; i < b.length; i++) {
            user.sendPM("  #" + b[i].ident + " " + b[i].name + " [" + b[i].externalIp + "]");
        }
        return;
    }

    user.sendPM("subcomandos: hablar | kick | setters | target | bans  (todos con <nick>)");
}

// ---------------------------------------------------------------- S-G
// El eval `@código` (solo Owner) corre en el primer script cargado, con
// `userobj` = quien lo escribió. Probá en el chat, como Owner:
//   @print("hola desde eval, soy " + userobj.name)
//   @userobj.sendPM("me hablo a mi mismo")

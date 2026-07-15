//! API expuesta a los scripts JS.
//!
//! Las native functions NO capturan estado en el closure (limitación de
//! `boa_engine 0.20` que requiere `Copy` para capturar). En su lugar,
//! usan un **registro global** que mapea `Context* → Arc<AppContext>`.
//! Cuando `make_context` se llama, registra el `Arc<AppContext>`.
//! Cuando la `Context` se destruye, hay que llamar `unregister_context`.
//!
//! ## API expuesta (global)
//!
//! ### Mensajería
//! - `print(msg)` — log a tracing
//! - `log(msg)` — log a tracing
//! - `sendPublic(from, text)` — broadcast de mensaje público
//! - `sendEmote(from, text)` — broadcast de emote
//! - `sendPM(from, to, text)` — mensaje privado a un user
//!
//! ### Usuarios
//! - `userCount()` — cantidad de users conectados
//! - `userNames()` — array de nicks conectados
//! - `userExists(name)` — true si el nick está conectado
//! - `getUserIp(name)` — IP externa del user
//! - `getUserLevel(name)` — nivel (0=Anon, 1=User, 2=Mod, 3=Admin, 4=Owner)
//! - `getUserVroom(name)` — vroom (0-65535)
//!
//! ### Sala
//! - `getTopic()` — topic actual
//! - `setTopic(topic)` — cambia el topic (visible para todos)
//! - `kickUser(name)` — desconecta al user
//!
//! ### Hashing
//! - `astraHash(s)` — SHA-1 hex
//! - `astraMd5(s)` — MD5 hex
//! - `astraBase64Encode(s)` / `astraBase64Decode(s)` — base64

use std::cell::RefCell;
use std::sync::Arc;

use boa_engine::{
    context::Context,
    js_string,
    native_function::NativeFunction,
    value::JsValue,
};
use bytes::Bytes;
use server_core::AppContext;

// ============================================================================
// Thread-local AppContext registry
// ============================================================================
//
// El `Context` de `boa_engine` no es `Send` y debe usarse en un único
// thread. El `ScriptManager` corre en un thread dedicado, y todos los
// scripts se ejecutan en ese thread. Por eso usamos un **thread-local**
// en vez de un HashMap global keyed por `Context*`: cuando `make_context`
// mueve el `Context` por valor, su dirección de memoria cambia, lo que
// invalidaría la clave. El thread-local es más simple y funciona.

thread_local! {
    static CURRENT_APP: RefCell<Option<Arc<AppContext>>> = const { RefCell::new(None) };
}

/// Registra el `Arc<AppContext>` para el thread actual.
/// Llamar desde `make_context` antes de retornar el `Context`.
pub fn register_context(_ctx: &Context, app: &Arc<AppContext>) {
    CURRENT_APP.with(|c| *c.borrow_mut() = Some(app.clone()));
}

/// Elimina el registro del thread actual.
pub fn unregister_context(_ctx: &Context) {
    CURRENT_APP.with(|c| *c.borrow_mut() = None);
}

/// Obtiene el `Arc<AppContext>` registrado en el thread actual.
fn lookup_app(_ctx: &Context) -> Option<Arc<AppContext>> {
    CURRENT_APP.with(|c| c.borrow().clone())
}

// ============================================================================
// Carpeta del script (modelo de carpetas, paridad sb0t)
// ============================================================================
//
// Cada script vive en su propia carpeta (`<scripts_dir>/<nombre>/`). El
// `include()` y las funciones `File_*` se resuelven RELATIVO a esa carpeta.
// La ruta de la carpeta se guarda como el global JS `__SCRIPT_DIR__` en el
// context de cada script (lo setea `load_source` al cargar), así cada script
// ve su propia carpeta cuando corre.

/// Setea la carpeta del script como global `__SCRIPT_DIR__` en su context.
/// Lo llama el manager al cargar (`load_source`).
pub fn set_script_dir(ctx: &mut Context, dir: &str) {
    let global = ctx.global_object();
    let _ = global.set(js_string!("__SCRIPT_DIR__"), JsValue::from(js_string!(dir)), false, ctx);
}

/// Registra el NOMBRE del script en su contexto. Sirve para atribuirle los
/// recursos globales que registre (líneas de `/help`, timers), y así poder
/// limpiarlos cuando el script se descargue o se recargue.
pub fn set_script_name(ctx: &mut Context, name: &str) {
    let global = ctx.global_object();
    let _ = global.set(js_string!("__SCRIPT_NAME__"), JsValue::from(js_string!(name)), false, ctx);
}

/// Nombre del script actual (lee `__SCRIPT_NAME__` del context).
fn current_script_name(ctx: &mut Context) -> String {
    let global = ctx.global_object();
    global
        .get(js_string!("__SCRIPT_NAME__"), ctx)
        .ok()
        .and_then(|v| jsvalue_to_string(&v))
        .unwrap_or_default()
}

/// Carpeta del script actual (lee `__SCRIPT_DIR__` del context).
fn current_script_dir(ctx: &mut Context) -> Option<std::path::PathBuf> {
    let global = ctx.global_object();
    let v = global.get(js_string!("__SCRIPT_DIR__"), ctx).ok()?;
    jsvalue_to_string(&v).map(std::path::PathBuf::from)
}

/// Resuelve un nombre de archivo relativo a la carpeta del script. Si `arg`
/// es una ruta absoluta se usa tal cual (retrocompat). Con `add_js`, agrega la
/// extensión `.js` si falta (para `include`). Nunca deja escapar de la carpeta
/// del script (rechaza rutas con `..`).
fn resolve_script_path(ctx: &mut Context, arg: &str, add_js: bool) -> Option<std::path::PathBuf> {
    if arg.is_empty() || arg.contains("..") {
        return None;
    }
    let mut name = arg.to_string();
    if add_js && !name.to_ascii_lowercase().ends_with(".js") {
        name.push_str(".js");
    }
    let p = std::path::Path::new(&name);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    let dir = current_script_dir(ctx)?;
    Some(dir.join(name))
}

// ============================================================================
// make_context
// ============================================================================

/// Crea un `Context` JS con el API completa inyectada.
pub fn make_context(app: Arc<AppContext>) -> Context {
    let mut context = Context::default();
    register_context(&context, &app);

    // ============ Logging ============

    context
        .register_global_builtin_callable(js_string!("print"), 1, NativeFunction::from_fn_ptr(print_fn))
        .expect("print should be registered");
    context
        .register_global_builtin_callable(js_string!("log"), 1, NativeFunction::from_fn_ptr(log_fn))
        .expect("log should be registered");

    // ============ Mensajería ============

    context
        .register_global_builtin_callable(js_string!("sendPublic"), 2, NativeFunction::from_fn_ptr(send_public_fn))
        .expect("sendPublic should be registered");
    context
        .register_global_builtin_callable(js_string!("sendEmote"), 2, NativeFunction::from_fn_ptr(send_emote_fn))
        .expect("sendEmote should be registered");
    context
        .register_global_builtin_callable(js_string!("__send_to_user"), 4, NativeFunction::from_fn_ptr(send_to_user_fn))
        .expect("__send_to_user should be registered");
    context
        .register_global_builtin_callable(js_string!("sendPM"), 3, NativeFunction::from_fn_ptr(send_pm_fn))
        .expect("sendPM should be registered");

    // ============ Usuarios ============

    context
        .register_global_builtin_callable(js_string!("userCount"), 0, NativeFunction::from_fn_ptr(user_count_fn))
        .expect("userCount should be registered");
    context
        .register_global_builtin_callable(js_string!("userNames"), 0, NativeFunction::from_fn_ptr(user_names_fn))
        .expect("userNames should be registered");
    context
        .register_global_builtin_callable(js_string!("userExists"), 1, NativeFunction::from_fn_ptr(user_exists_fn))
        .expect("userExists should be registered");
    context
        .register_global_builtin_callable(js_string!("getUserIp"), 1, NativeFunction::from_fn_ptr(get_user_ip_fn))
        .expect("getUserIp should be registered");
    context
        .register_global_builtin_callable(js_string!("getUserLevel"), 1, NativeFunction::from_fn_ptr(get_user_level_fn))
        .expect("getUserLevel should be registered");
    context
        .register_global_builtin_callable(js_string!("getUserVroom"), 1, NativeFunction::from_fn_ptr(get_user_vroom_fn))
        .expect("getUserVroom should be registered");
    context
        .register_global_builtin_callable(js_string!("kickUser"), 1, NativeFunction::from_fn_ptr(kick_user_fn))
        .expect("kickUser should be registered");

    // ============ Sala ============

    context
        .register_global_builtin_callable(js_string!("getTopic"), 0, NativeFunction::from_fn_ptr(get_topic_fn))
        .expect("getTopic should be registered");
    context
        .register_global_builtin_callable(js_string!("setTopic"), 1, NativeFunction::from_fn_ptr(set_topic_fn))
        .expect("setTopic should be registered");

    // ============ Objeto user / Room / Stats (compat sb0t) ============

    context
        .register_global_builtin_callable(js_string!("__user_get"), 2, NativeFunction::from_fn_ptr(user_get_fn))
        .expect("__user_get should be registered");
    context
        .register_global_builtin_callable(js_string!("__user_do"), 3, NativeFunction::from_fn_ptr(user_do_fn))
        .expect("__user_do should be registered");
    context
        .register_global_builtin_callable(js_string!("__room_get"), 1, NativeFunction::from_fn_ptr(room_get_fn))
        .expect("__room_get should be registered");
    context
        .register_global_builtin_callable(js_string!("__stats_get"), 1, NativeFunction::from_fn_ptr(stats_get_fn))
        .expect("__stats_get should be registered");
    context
        .register_global_builtin_callable(js_string!("__records_json"), 0, NativeFunction::from_fn_ptr(records_json_fn))
        .expect("__records_json should be registered");
    context
        .register_global_builtin_callable(js_string!("__banned_json"), 0, NativeFunction::from_fn_ptr(banned_json_fn))
        .expect("__banned_json should be registered");
    context
        .register_global_builtin_callable(js_string!("__unban_ident"), 1, NativeFunction::from_fn_ptr(unban_ident_fn))
        .expect("__records_json should be registered");
    context
        .register_global_builtin_callable(js_string!("__record_ban"), 6, NativeFunction::from_fn_ptr(record_ban_fn))
        .expect("__record_ban should be registered");

    // ============ Hashing ============

    context
        .register_global_builtin_callable(js_string!("astraHash"), 1, NativeFunction::from_fn_ptr(hash_sha1_fn))
        .expect("astraHash should be registered");
    context
        .register_global_builtin_callable(js_string!("astraMd5"), 1, NativeFunction::from_fn_ptr(hash_md5_fn))
        .expect("astraMd5 should be registered");
    context
        .register_global_builtin_callable(js_string!("astraBase64Encode"), 1, NativeFunction::from_fn_ptr(b64_enc_fn))
        .expect("astraBase64Encode should be registered");
    context
        .register_global_builtin_callable(js_string!("astraBase64Decode"), 1, NativeFunction::from_fn_ptr(b64_dec_fn))
        .expect("astraBase64Decode should be registered");

    // ============ File I/O ============

    context
        .register_global_builtin_callable(js_string!("File_exists"), 1, NativeFunction::from_fn_ptr(file_exists_fn))
        .expect("File_exists should be registered");
    context
        .register_global_builtin_callable(js_string!("File_size"), 1, NativeFunction::from_fn_ptr(file_size_fn))
        .expect("File_size should be registered");
    context
        .register_global_builtin_callable(js_string!("File_creationTime"), 1, NativeFunction::from_fn_ptr(file_creation_time_fn))
        .expect("File_creationTime should be registered");
    context
        .register_global_builtin_callable(js_string!("File_read"), 1, NativeFunction::from_fn_ptr(file_read_fn))
        .expect("File_read should be registered");
    context
        .register_global_builtin_callable(js_string!("File_write"), 2, NativeFunction::from_fn_ptr(file_write_fn))
        .expect("File_write should be registered");
    context
        .register_global_builtin_callable(js_string!("File_append"), 2, NativeFunction::from_fn_ptr(file_append_fn))
        .expect("File_append should be registered");
    context
        .register_global_builtin_callable(js_string!("File_delete"), 1, NativeFunction::from_fn_ptr(file_delete_fn))
        .expect("File_delete should be registered");
    context
        .register_global_builtin_callable(js_string!("__read_file_b64"), 1, NativeFunction::from_fn_ptr(read_file_b64_fn))
        .expect("__read_file_b64 should be registered");

    // ============ Compresión ============

    context
        .register_global_builtin_callable(js_string!("Zip_compress"), 1, NativeFunction::from_fn_ptr(zip_compress_fn))
        .expect("Zip_compress should be registered");
    context
        .register_global_builtin_callable(js_string!("Zip_decompress"), 1, NativeFunction::from_fn_ptr(zip_decompress_fn))
        .expect("Zip_decompress should be registered");

    // ============ Script include (modelo de carpetas, paridad sb0t) ============

    context
        .register_global_builtin_callable(js_string!("ScriptInclude_run"), 1, NativeFunction::from_fn_ptr(script_include_fn))
        .expect("ScriptInclude_run should be registered");
    // Alias sb0t: `include("sub")` carga `<carpeta_del_script>/sub.js`.
    context
        .register_global_builtin_callable(js_string!("include"), 1, NativeFunction::from_fn_ptr(script_include_fn))
        .expect("include should be registered");
    // `includeAll()` carga todos los `.js` de la carpeta salvo el principal.
    context
        .register_global_builtin_callable(js_string!("includeAll"), 0, NativeFunction::from_fn_ptr(include_all_fn))
        .expect("includeAll should be registered");

    // ============ Spell check ============

    context
        .register_global_builtin_callable(js_string!("Spelling_check"), 1, NativeFunction::from_fn_ptr(spelling_check_fn))
        .expect("Spelling_check should be registered");

    // ============ Compatibilidad sb0t: mismos nombres que el original ============
    //
    // Los scripts sb0t legados usan estos nombres. Algunos son alias de las
    // funciones modernas; otros son stubs honestos (retornan default y loguean
    // un warning, marcado con ⚠️ en el docstring).

    // --- Aliases (delegan a la implementación moderna) ---
    context
        .register_global_builtin_callable(js_string!("Base64_encode"), 1, NativeFunction::from_fn_ptr(b64_enc_fn))
        .expect("Base64_encode should be registered");
    context
        .register_global_builtin_callable(js_string!("Base64_decode"), 1, NativeFunction::from_fn_ptr(b64_dec_fn))
        .expect("Base64_decode should be registered");
    context
        .register_global_builtin_callable(js_string!("Crypto_hashSHA1"), 1, NativeFunction::from_fn_ptr(hash_sha1_fn))
        .expect("Crypto_hashSHA1 should be registered");
    context
        .register_global_builtin_callable(js_string!("Crypto_hashMD5"), 1, NativeFunction::from_fn_ptr(hash_md5_fn))
        .expect("Crypto_hashMD5 should be registered");
    context
        .register_global_builtin_callable(js_string!("Users_count"), 0, NativeFunction::from_fn_ptr(user_count_fn))
        .expect("Users_count should be registered");
    context
        .register_global_builtin_callable(js_string!("Room_setTopic"), 1, NativeFunction::from_fn_ptr(set_topic_fn))
        .expect("Room_setTopic should be registered");

    // --- Stubs honestos (⚠️ comportamiento parcial o default) ---
    context
        .register_global_builtin_callable(js_string!("Channels_list"), 0, NativeFunction::from_fn_ptr(channels_list_fn))
        .expect("Channels_list should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_get"), 1, NativeFunction::from_fn_ptr(channels_get_fn))
        .expect("Channels_get should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_create"), 2, NativeFunction::from_fn_ptr(channels_create_fn))
        .expect("Channels_create should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_setTopic"), 2, NativeFunction::from_fn_ptr(channels_set_topic_fn))
        .expect("Channels_setTopic should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_broadcast"), 3, NativeFunction::from_fn_ptr(channels_broadcast_fn))
        .expect("Channels_broadcast should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_kick"), 2, NativeFunction::from_fn_ptr(channels_kick_fn))
        .expect("Channels_kick should be registered");
    context
        .register_global_builtin_callable(js_string!("Channels_delete"), 1, NativeFunction::from_fn_ptr(channels_delete_fn))
        .expect("Channels_delete should be registered");
    context
        .register_global_builtin_callable(js_string!("Hashlink_create"), 2, NativeFunction::from_fn_ptr(hashlink_create_fn))
        .expect("Hashlink_create should be registered");
    context
        .register_global_builtin_callable(js_string!("Hashlink_parse"), 1, NativeFunction::from_fn_ptr(hashlink_parse_fn))
        .expect("Hashlink_parse should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_list"), 0, NativeFunction::from_fn_ptr(link_list_fn))
        .expect("Link_list should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_getUserList"), 0, NativeFunction::from_fn_ptr(link_get_user_list_fn))
        .expect("Link_getUserList should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_disconnect"), 1, NativeFunction::from_fn_ptr(link_disconnect_fn_real))
        .expect("Link_disconnect should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_findLeaf"), 1, NativeFunction::from_fn_ptr(link_find_leaf_fn_real))
        .expect("Link_findLeaf should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_findUser"), 1, NativeFunction::from_fn_ptr(link_find_user_fn_real))
        .expect("Link_findUser should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_findHub"), 1, NativeFunction::from_fn_ptr(link_find_hub_fn_real))
        .expect("Link_findHub should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_kickHub"), 1, NativeFunction::from_fn_ptr(link_kick_hub_fn_real))
        .expect("Link_kickHub should be registered");
    context
        .register_global_builtin_callable(js_string!("Users_getUserByName"), 1, NativeFunction::from_fn_ptr(users_get_by_name_fn))
        .expect("Users_getUserByName should be registered");
    context
        .register_global_builtin_callable(js_string!("Stats_addStat"), 2, NativeFunction::from_fn_ptr(stats_add_stat_fn))
        .expect("Stats_addStat should be registered");
    context
        .register_global_builtin_callable(js_string!("Stats_getStat"), 1, NativeFunction::from_fn_ptr(stats_get_stat_fn))
        .expect("Stats_getStat should be registered");
    context
        .register_global_builtin_callable(js_string!("Entities_list"), 0, NativeFunction::from_fn_ptr(entities_list_fn))
        .expect("Entities_list should be registered");
    context
        .register_global_builtin_callable(js_string!("Link_createLink"), 2, NativeFunction::from_fn_ptr(link_create_link_fn))
        .expect("Link_createLink should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_createKey"), 1, NativeFunction::from_fn_ptr(registry_create_key_fn))
        .expect("Registry_createKey should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_deleteKey"), 1, NativeFunction::from_fn_ptr(registry_delete_key_fn))
        .expect("Registry_deleteKey should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_getValue"), 1, NativeFunction::from_fn_ptr(registry_get_value_fn))
        .expect("Registry_getValue should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_setValue"), 2, NativeFunction::from_fn_ptr(registry_set_value_fn))
        .expect("Registry_setValue should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_exists"), 1, NativeFunction::from_fn_ptr(registry_exists_fn))
        .expect("Registry_exists should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_getKeys"), 0, NativeFunction::from_fn_ptr(registry_get_keys_fn))
        .expect("Registry_getKeys should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_deleteValue"), 1, NativeFunction::from_fn_ptr(registry_delete_value_fn))
        .expect("Registry_deleteValue should be registered");
    context
        .register_global_builtin_callable(js_string!("Registry_clear"), 0, NativeFunction::from_fn_ptr(registry_clear_fn))
        .expect("Registry_clear should be registered");
    context
        .register_global_builtin_callable(js_string!("Room_broadcast"), 1, NativeFunction::from_fn_ptr(room_broadcast_fn))
        .expect("Room_broadcast should be registered");

    // ============ Avatar / object class (Fase 18) ============

    context
        .register_global_builtin_callable(js_string!("Avatar_new"), 1, NativeFunction::from_fn_ptr(avatar_new_fn))
        .expect("Avatar_new should be registered");
    context
        .register_global_builtin_callable(js_string!("Avatar_getSize"), 1, NativeFunction::from_fn_ptr(avatar_get_size_fn))
        .expect("Avatar_getSize should be registered");
    context
        .register_global_builtin_callable(js_string!("Avatar_setForUser"), 2, NativeFunction::from_fn_ptr(avatar_set_for_user_fn))
        .expect("Avatar_setForUser should be registered");
    context
        .register_global_builtin_callable(js_string!("Avatar_getForUser"), 1, NativeFunction::from_fn_ptr(avatar_get_for_user_fn))
        .expect("Avatar_getForUser should be registered");
    context
        .register_global_builtin_callable(js_string!("Avatar_save"), 2, NativeFunction::from_fn_ptr(avatar_save_fn))
        .expect("Avatar_save should be registered");
    context
        .register_global_builtin_callable(js_string!("Avatar_getBytes"), 1, NativeFunction::from_fn_ptr(avatar_get_bytes_fn))
        .expect("Avatar_getBytes should be registered");

    // ============ ScribbleImage (Fase 18) ============

    context
        .register_global_builtin_callable(js_string!("ScribbleImage_new"), 1, NativeFunction::from_fn_ptr(scribble_image_new_fn))
        .expect("ScribbleImage_new should be registered");
    context
        .register_global_builtin_callable(js_string!("ScribbleImage_getSize"), 1, NativeFunction::from_fn_ptr(scribble_image_get_size_fn))
        .expect("ScribbleImage_getSize should be registered");
    context
        .register_global_builtin_callable(js_string!("ScribbleImage_save"), 2, NativeFunction::from_fn_ptr(scribble_image_save_fn))
        .expect("ScribbleImage_save should be registered");

    // ============ Spell suggest + Query (Fase 19) ============

    context
        .register_global_builtin_callable(js_string!("Spelling_suggest"), 1, NativeFunction::from_fn_ptr(spelling_suggest_fn))
        .expect("Spelling_suggest should be registered");
    context
        .register_global_builtin_callable(js_string!("Spelling_confirm"), 1, NativeFunction::from_fn_ptr(spelling_confirm_fn))
        .expect("Spelling_confirm should be registered");
    context
        .register_global_builtin_callable(js_string!("Query_new"), 1, NativeFunction::from_fn_ptr(query_new_fn))
        .expect("Query_new should be registered");
    context
        .register_global_builtin_callable(js_string!("Query_getResults"), 1, NativeFunction::from_fn_ptr(query_get_results_fn))
        .expect("Query_getResults should be registered");
    context
        .register_global_builtin_callable(js_string!("Query_getColumnCount"), 1, NativeFunction::from_fn_ptr(query_get_column_count_fn))
        .expect("Query_getColumnCount should be registered");
    context
        .register_global_builtin_callable(js_string!("Query_getRowCount"), 1, NativeFunction::from_fn_ptr(query_get_row_count_fn))
        .expect("Query_getRowCount should be registered");

    // ============ Help extend (Fase 20) ============

    context
        .register_global_builtin_callable(js_string!("Help_addLine"), 2, NativeFunction::from_fn_ptr(help_add_line_fn))
        .expect("Help_addLine should be registered");

    // ============ Timer (Fase 20) ============

    context
        .register_global_builtin_callable(js_string!("setTimer"), 2, NativeFunction::from_fn_ptr(set_timer_fn))
        .expect("setTimer should be registered");
    context
        .register_global_builtin_callable(js_string!("setTimeout"), 2, NativeFunction::from_fn_ptr(set_timeout_fn))
        .expect("setTimeout should be registered");
    context
        .register_global_builtin_callable(js_string!("clearTimer"), 1, NativeFunction::from_fn_ptr(clear_timer_fn))
        .expect("clearTimer should be registered");
    context
        .register_global_builtin_callable(js_string!("__http_download"), 8, NativeFunction::from_fn_ptr(http_download_fn))
        .expect("__http_download should be registered");

    // ============ Sql (DB propia del script, paridad sb0t) ============
    for (name, arity, f) in [
        ("__Sql_new", 0usize, sql_new_fn as fn(&JsValue, &[JsValue], &mut Context) -> Result<JsValue, boa_engine::JsError>),
        ("__Sql_open", 2, sql_open_fn),
        ("__Sql_query", 3, sql_query_fn),
        ("__Sql_canRead", 1, sql_can_read_fn),
        ("__Sql_value", 2, sql_value_fn),
        ("__Sql_close", 1, sql_close_fn),
        ("__Sql_lastError", 1, sql_last_error_fn),
    ] {
        context
            .register_global_builtin_callable(js_string!(name), arity, NativeFunction::from_fn_ptr(f))
            .unwrap_or_else(|_| panic!("{} should be registered", name));
    }

    // ============ Capa de compatibilidad sb0t ============
    // Define los objetos/constructores/globals que usan los scripts REALES de
    // sb0t (`Room.setTopic()`, `new Query()`, `new Sql()`, `Users.getUserByName()`,
    // etc.) mapeándolos a las funciones planas que Astra ya expone. Fase 1: se
    // cubren las que existen; las que faltan se irán agregando por fases.
    if let Err(e) = context.eval(boa_engine::Source::from_bytes(SB0T_COMPAT_PRELUDE.as_bytes())) {
        tracing::error!("error cargando el prelude de compatibilidad sb0t: {}", e);
    }

    context
}

/// Prelude JS de compatibilidad con la API de scripts de sb0t. Se evalúa en
/// cada context antes del código del script.
const SB0T_COMPAT_PRELUDE: &str = r#"
// ---- Objeto user (JSUser): propiedades vivas + métodos ----
var __USER_PROPS = ["name","orgName","id","level","vroom","externalIp","localIp",
  "dns","guid","version","age","gender","sex","country","region","fileCount","port",
  "muzzled","cloaked","registered","encrypted","owner","webClient","customClient",
  "browsable","fastPing","canHTML","personalMessage","customName","joinTime",
  "captcha","idle","visible","ghost","localEP","linked","leaf"];
function __mkUser(name){
  if (name == null) return null;
  var u = { __name: "" + name };
  __USER_PROPS.forEach(function(p){
    Object.defineProperty(u, p, {
      enumerable: true,
      configurable: true,
      get: function(){ return __user_get(u.__name, p); }
    });
  });
  u.ban = function(){ return __user_do(u.__name, "ban", ""); };
  u.kick = function(){ return __user_do(u.__name, "kick", ""); };
  u.disconnect = function(){ return __user_do(u.__name, "disconnect", ""); };
  u.sendText = function(t){ return __user_do(u.__name, "sendText", t == null ? "" : "" + t); };
  u.sendPM   = function(t){ return __user_do(u.__name, "sendPM",   t == null ? "" : "" + t); };
  u.sendHTML = function(t){ return __user_do(u.__name, "sendHTML", t == null ? "" : "" + t); };
  u.sendEmote = function(t){ return __user_do(u.__name, "sendEmote", t == null ? "" : "" + t); };
  u.exists = function(){ return userExists(u.__name); };
  // Setters writable (paridad sb0t: u.customName = "X", u.vroom = 2, u.level = 1)
  ["customName","vroom","level","muzzled"].forEach(function(p){
    Object.defineProperty(u, p, {
      enumerable: true,
      get: function(){ return __user_get(u.__name, p); },
      set: function(v){ __user_do(u.__name, "set:" + p, v == null ? "" : "" + v); }
    });
  });
  // avatar: get = base64 (o null); set = base64 (vacío/null limpia).
  Object.defineProperty(u, "avatar", {
    enumerable: true, configurable: true,
    get: function(){ return Avatar_getForUser(u.__name); },
    set: function(v){ __user_do(u.__name, "set:avatar", v == null ? "" : "" + v); }
  });
  // font: objeto {name,size,color,bold,italic,underline} (solo lectura:
  // la fuente la fija el cliente en el login; el set es no-op).
  Object.defineProperty(u, "font", {
    enumerable: true, configurable: true,
    get: function(){
      try { return JSON.parse(__user_get(u.__name, "fontJson")); } catch (e) { return null; }
    },
    set: function(v){ /* no-op: fuente fijada por el cliente */ }
  });
  // Métodos sb0t restantes
  u.redirect = function(link){ return __user_do(u.__name, "redirect", link == null ? "" : "" + link); };
  u.setTopic = function(t){ return __user_do(u.__name, "setTopic", t == null ? "" : "" + t); };
  u.nudge = function(){ return __user_do(u.__name, "nudge", ""); };
  u.setUrl = function(){ return false; };      // stub: URL por-usuario no soportada
  u.scribble = function(){ return false; };    // stub: scribble dirigido no soportado
  u.restoreAvatar = function(){ return false; };
  u.getASN = function(){ return null; };       // stub: sin base ASN
  u.ignores = function(){ try { return JSON.parse(__user_get(u.__name, "ignoresJson") || "[]"); } catch (e) { return []; } };
  // En contexto string el objeto se comporta como su nombre: mantiene
  // compat con handlers "nativos" de Astra que usaban el nombre (string)
  // como primer argumento (concatenación, ==, plantillas).
  u.toString = function(){ return u.__name; };
  u.valueOf = function(){ return u.__name; };
  return u;
}
function user(name){ return __mkUser(name); }

// ---- Objetos estáticos (mapean funciones planas de Astra) ----
var Room = {
  setTopic: Room_setTopic, broadcast: Room_broadcast, topic: getTopic,
  name: function(){ return __room_get("name"); },
  botName: function(){ return __room_get("botName"); },
  port: function(){ return __room_get("port"); },
  version: function(){ return __room_get("version"); },
  externalIp: function(){ return __room_get("externalIp"); },
  startTime: function(){ return __room_get("startTime"); }
};
var Users = {
  count: Users_count, getUserByName: function(n){ return __mkUser(n); },
  exists: userExists, names: userNames,
  local: function(){ var r = userNames(); return (r || []).map(__mkUser); },
  // Historial de usuarios desconectados (JSRecord con ban()).
  records: function(){
    var arr;
    try { arr = JSON.parse(__records_json()); } catch (e) { return []; }
    return arr.map(function(r){
      r.ban = function(){ return __record_ban(r.name, r.version, r.guid, r.externalIp, r.localIp, r.port); };
      return r;
    });
  },
  // JSBannedUser reales (con unban()), sobre la ban list del server.
  banned: function(){
    var arr;
    try { arr = JSON.parse(__banned_json()); } catch (e) { return []; }
    return arr.map(function(b){
      b.unban = function(){ return __unban_ident(b.ident); };
      return b;
    });
  },
  // Usuarios remotos vía link: (linkName, userName) → objetos user.
  linked: function(){
    var arr;
    try { arr = JSON.parse(Link_list()); } catch (e) { return []; }
    return Array.isArray(arr) ? arr : [];
  }
};
var Channels = { create: Channels_create, "delete": Channels_delete,
                 get: function(id){ try { return JSON.parse(Channels_get(id)); } catch (e) { return null; } },
                 list: function(){ try { return JSON.parse(Channels_list()); } catch (e) { return []; } },
                 available: function(){ try { return JSON.parse(Channels_list()); } catch (e) { return []; } },
                 enabled: function(){ return true; },
                 search: function(id){ try { return JSON.parse(Channels_get(id)); } catch (e) { return null; } },
                 broadcast: Channels_broadcast, setTopic: Channels_setTopic, kick: Channels_kick };
var Base64 = { encode: Base64_encode, decode: Base64_decode };
var Zip = { compress: Zip_compress, uncompress: Zip_decompress, decompress: Zip_decompress };
var Hashlink = { create: Hashlink_create, parse: Hashlink_parse, encode: Hashlink_create, decode: Hashlink_parse };
var Entities = {
  list: Entities_list,
  encode: function(s){ return ("" + (s == null ? "" : s)).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;").replace(/'/g,"&#39;"); },
  decode: function(s){ return ("" + (s == null ? "" : s)).replace(/&#0?39;/g,"'").replace(/&apos;/g,"'").replace(/&quot;/g,'"').replace(/&gt;/g,">").replace(/&lt;/g,"<").replace(/&amp;/g,"&"); }
};
var Spelling = { check: Spelling_check, suggest: Spelling_suggest, confirm: Spelling_confirm };
var Stats = {
  addStat: Stats_addStat, getStat: Stats_getStat,
  userCount: function(){ return __stats_get("userCount"); },
  peakUserCount: function(){ return __stats_get("peakUserCount"); },
  joinCount: function(){ return __stats_get("joinCount"); },
  partCount: function(){ return __stats_get("partCount"); },
  dataReceived: function(){ return __stats_get("dataReceived"); },
  dataSent: function(){ return __stats_get("dataSent"); },
  floodCount: function(){ return __stats_get("floodCount"); },
  invalidLoginCount: function(){ return __stats_get("invalidLoginCount"); },
  rejectionCount: function(){ return __stats_get("rejectionCount"); },
  messageCount: function(){ return __stats_get("messageCount"); },
  pmCount: function(){ return __stats_get("pmCount"); }
};
var Registry = {
  createKey: Registry_createKey, deleteKey: Registry_deleteKey,
  exists: Registry_exists, getValue: Registry_getValue, getKeys: Registry_getKeys,
  setValue: Registry_setValue, deleteValue: Registry_deleteValue, clear: Registry_clear
};
var Crypto = { hashSHA1: Crypto_hashSHA1, hashMD5: Crypto_hashMD5, sha1: Crypto_hashSHA1, md5: Crypto_hashMD5 };
var Link = { createLink: Link_createLink, connect: Link_createLink, disconnect: Link_disconnect,
             findHub: Link_findHub, findLeaf: Link_findLeaf, findUser: Link_findUser,
             getUserList: Link_getUserList, kickHub: Link_kickHub, list: Link_list,
             // sb0t: getters de estado del link (Astra no linkea multi-servidor → defaults).
             leaves: function(){ try { return JSON.parse(Link_list()); } catch (e) { return []; } },
             leaf: function(){ return null; },
             linked: function(){ try { return JSON.parse(Link_list()).length > 0; } catch (e) { return false; } },
             name: function(){ return ""; }, externalIp: function(){ return ""; },
             port: function(){ return 0; }, hashlink: function(){ return ""; } };
var File = {
  exists: File_exists, load: File_read, read: File_read, save: File_write, write: File_write,
  append: File_append, appendLine: function(n, t){ return File_append(n, (t == null ? "" : t) + "\r\n"); },
  kill: File_delete, "delete": File_delete
};

// ---- Puente de nombres de handler Astra→sb0t ----
// Astra dispara onPublic/onEmote; los scripts de sb0t definen
// onTextReceived/onEmoteReceived. Si el script NO define el handler nativo de
// Astra (lo sobreescribiría, ya que corre después del prelude), reenviamos al
// nombre de sb0t. El primer arg ya es un objeto `user` (Fase 4).
function onPublic(user, text){ if (typeof onTextReceived === "function") return onTextReceived(user, text); }
function onEmote(user, text){ if (typeof onEmoteReceived === "function") return onEmoteReceived(user, text); }
// Astra dispara onPrivate(emisor, destino, texto); sb0t define onPM(emisor,
// destino) — ambos JSUser. Reenviamos si el script no definió onPrivate.
function onPrivate(from, to, text){ if (typeof onPM === "function") return onPM(from, to); }

// ---- Globals sb0t ----
// sb0t: sendText/sendEmote/sendPM(user, sender, text) → a ESE usuario.
// Astra nativo: sendPublic/sendEmote(from, text) broadcast, sendPM(from, to, text).
// Los wrappers detectan la forma sb0t (primer arg = objeto user) y si no,
// caen al comportamiento nativo de Astra.
// OJO: usar EXPRESIONES de función (asignaciones), no declaraciones — las
// declaraciones se hoisten y `__sendXRaw = sendX` capturaría el wrapper en vez
// del native (recursión infinita). Las asignaciones respetan el orden: primero
// se captura el native, después se reasigna el global.
var __sendPublicRaw = sendPublic;
var __sendEmoteRaw = sendEmote;
var __sendPMRaw = sendPM;
var __isUserObj = function(a){ return a != null && typeof a === "object" && a.__name !== undefined; };
sendText = function(a, b, c){
  if (__isUserObj(a)) return __send_to_user(a.__name, "public", b == null ? "" : "" + b, c == null ? "" : "" + c);
  return __sendPublicRaw(a == null ? "" : "" + a, b == null ? "" : "" + b);
};
sendEmote = function(a, b, c){
  if (__isUserObj(a)) return __send_to_user(a.__name, "emote", b == null ? "" : "" + b, c == null ? "" : "" + c);
  return __sendEmoteRaw(a == null ? "" : "" + a, b == null ? "" : "" + b);
};
sendPM = function(a, b, c){
  if (__isUserObj(a)) return __send_to_user(a.__name, "pm", b == null ? "" : "" + b, c == null ? "" : "" + c);
  return __sendPMRaw(a == null ? "" : "" + a, b == null ? "" : "" + b, c == null ? "" : "" + c);
};
function scriptName(){ return (typeof __SCRIPT_DIR__ === "string") ? __SCRIPT_DIR__.replace(/[\\/]+$/,"").split(/[\\/]/).pop() : ""; }
function tickCount(){ return Date.now(); }
function byteLength(s){ s = (s == null ? "" : "" + s); var n = 0; for (var i = 0; i < s.length; i++){ var c = s.charCodeAt(i); n += c < 0x80 ? 1 : c < 0x800 ? 2 : 3; } return n; }
function stripColors(s){ return ("" + (s == null ? "" : s)).replace(/\x03[0-9]{0,2}(,[0-9]{1,2})?/g, ""); }
function escapeUtf(s){ return encodeURIComponent(s == null ? "" : "" + s); }
function clrName(name){ return stripColors(name); }

// ---- List: colección tipada estilo sb0t (implementación pura JS) ----
function List(){ this.__a = []; }
Object.defineProperty(List.prototype, "count",  { get: function(){ return this.__a.length; } });
Object.defineProperty(List.prototype, "length", { get: function(){ return this.__a.length; } });
List.prototype.clear = function(){ this.__a = []; };
List.prototype.reverse = function(){ this.__a.reverse(); return this; };
List.prototype.sort = function(f){ this.__a.sort(f); return this; };
List.prototype.add = function(x){ this.__a.push(x); return this; };
List.prototype.addRange = function(arr){ for (var i = 0; i < arr.length; i++) this.__a.push(arr[i]); return this; };
List.prototype.insert = function(i, x){ this.__a.splice(i, 0, x); return this; };
List.prototype.insertRange = function(i, arr){ this.__a.splice.apply(this.__a, [i, 0].concat(arr)); return this; };
List.prototype.remove = function(x){ var i = this.__a.indexOf(x); if (i >= 0) this.__a.splice(i, 1); return i >= 0; };
List.prototype.removeAt = function(i){ this.__a.splice(i, 1); return this; };
List.prototype.removeRange = function(i, n){ this.__a.splice(i, n); return this; };
List.prototype.removeAll = function(f){ this.__a = this.__a.filter(function(x){ return !f(x); }); return this; };
List.prototype.getRange = function(i, n){ return this.__a.slice(i, i + n); };
List.prototype.get = function(i){ return this.__a[i]; };
List.prototype.indexOf = function(x){ return this.__a.indexOf(x); };
List.prototype.lastIndexOf = function(x){ return this.__a.lastIndexOf(x); };
List.prototype.find = function(f){ for (var i = 0; i < this.__a.length; i++) if (f(this.__a[i])) return this.__a[i]; return null; };
List.prototype.findAll = function(f){ return this.__a.filter(f); };
List.prototype.findIndex = function(f){ for (var i = 0; i < this.__a.length; i++) if (f(this.__a[i])) return i; return -1; };
List.prototype.findLastIndex = function(f){ for (var i = this.__a.length - 1; i >= 0; i--) if (f(this.__a[i])) return i; return -1; };
List.prototype.join = function(s){ return this.__a.join(s == null ? "," : s); };

// ---- Timer: repetitivo con oncomplete, sobre setTimer/clearTimer nativos ----
// setTimer() de Astra ya es repetitivo (el manager lo re-arma) y dispara el
// handler global onTimer(id, name). Registramos el callback por 'name' y lo
// enrutamos con un onTimer del prelude. Un script que defina su propio
// onTimer lo sobreescribe (sólo relevante si además usa el objeto Timer).
var __timerSeq = 0;
var __timerCbs = {};
function onTimer(id, name){ var f = __timerCbs[name]; if (typeof f === "function") { try { f(); } catch (e) {} } }
function Timer(){ this.interval = 1000; this.oncomplete = null; this.__id = null; this.__key = "__timer_" + (++__timerSeq); }
Timer.prototype.start = function(){
  var self = this;
  __timerCbs[this.__key] = function(){ if (typeof self.oncomplete === "function") self.oncomplete(); };
  this.__id = setTimer(Math.max(1, Math.round(this.interval / 1000)), this.__key);
  return this;
};
Timer.prototype.stop = function(){
  if (this.__id != null) { clearTimer(this.__id); this.__id = null; }
  delete __timerCbs[this.__key];
  return this;
};

// ---- Avatar: imagen de avatar sobre los natives Avatar_* ----
function Avatar(src){ this.oncomplete = null; this.__id = -1; if (src != null) this.__id = Avatar_new("" + src); }
Object.defineProperty(Avatar.prototype, "src", {
  get: function(){ return this.__id < 0 ? null : Avatar_getBytes(this.__id); },
  set: function(v){ this.__id = Avatar_new(v == null ? "" : "" + v); }
});
Object.defineProperty(Avatar.prototype, "size", { get: function(){ return this.__id < 0 ? -1 : Avatar_getSize(this.__id); } });
Avatar.prototype.save = function(path){ return this.__id < 0 ? false : Avatar_save(this.__id, "" + path); };
Avatar.prototype.load = function(path){ var b = __read_file_b64("" + path); if (b == null) return false; this.__id = Avatar_new(b); return this.__id >= 0; };
Avatar.prototype.setForUser = function(name){ return this.__id < 0 ? false : Avatar_setForUser("" + name, this.__id); };
Avatar.prototype.download = function(url){ if (typeof HttpRequest === "undefined") return false; var self = this; var r = new HttpRequest(); r.src = url; r.oncomplete = function(bytes){ if (bytes != null){ self.__id = Avatar_new(Base64_encode(bytes)); } if (typeof self.oncomplete === "function") self.oncomplete(self); }; return r.download(); };

// ---- Scribble: imagen scribble sobre los natives ScribbleImage_* ----
function Scribble(src){ this.oncomplete = null; this.__id = -1; if (src != null) this.__id = ScribbleImage_new("" + src); }
Object.defineProperty(Scribble.prototype, "src", {
  get: function(){ return this.__id < 0 ? null : this.__id; },
  set: function(v){ this.__id = ScribbleImage_new(v == null ? "" : "" + v); }
});
Object.defineProperty(Scribble.prototype, "size", { get: function(){ return this.__id < 0 ? -1 : ScribbleImage_getSize(this.__id); } });
Scribble.prototype.save = function(path){ return this.__id < 0 ? false : ScribbleImage_save(this.__id, "" + path); };
Scribble.prototype.load = function(path){ var b = __read_file_b64("" + path); if (b == null) return false; this.__id = ScribbleImage_new(b); return this.__id >= 0; };
Scribble.prototype.download = function(url){ if (typeof HttpRequest === "undefined") return false; var self = this; var r = new HttpRequest(); r.src = url; r.oncomplete = function(bytes){ if (bytes != null){ self.__id = ScribbleImage_new(Base64_encode(bytes)); } if (typeof self.oncomplete === "function") self.oncomplete(self); }; return r.download(); };

// ---- HttpRequest: petición HTTP async con oncomplete (Fase 3b) ----
// __http_download hace la petición en un thread de background; el manager
// entrega el resultado vía onHttpComplete(key, body, status, error), que
// enruta al callback registrado por 'key' (sólo existe en este context).
var __httpSeq = 0;
var __httpCbs = {};
function onHttpComplete(key, body, statusStr, error){
  var cb = __httpCbs[key];
  if (typeof cb === "function"){ delete __httpCbs[key]; cb(body, parseInt(statusStr, 10) || 0, error || ""); }
}
function HttpRequest(){
  this.method = "GET"; this.src = ""; this.host = ""; this.params = "";
  this.userAgent = ""; this.accept = ""; this.utf = true; this.oncomplete = null;
  this.response = null; this.status = 0; this.error = "";
  this.__headers = {};
}
// Header custom (paridad sb0t `req.header(nombre, valor)`).
HttpRequest.prototype.header = function(n, v){
  if (n != null) this.__headers["" + n] = v == null ? "" : "" + v;
  return this;
};
HttpRequest.prototype.download = function(){
  var self = this;
  var url = this.src || this.host;
  var m = ("" + this.method).toUpperCase();
  if (this.params && m === "GET"){ url += (url.indexOf("?") < 0 ? "?" : "&") + this.params; }
  var key = "__http_" + (++__httpSeq);
  __httpCbs[key] = function(body, status, error){
    self.response = body; self.status = status; self.error = error;
    if (typeof self.oncomplete === "function") self.oncomplete(body, status, error);
  };
  var body = (m === "POST") ? ("" + (this.params || "")) : "";
  var hdrs = ""; try { hdrs = JSON.stringify(this.__headers || {}); } catch (e) {}
  var ok = __http_download(m, "" + url, body, "" + this.userAgent, "" + this.accept, !!this.utf, key, hdrs);
  if (!ok) delete __httpCbs[key];
  return ok;
};

// ---- ProxyCheck: detección de proxy/VPN vía proxycheck.io (compat sb0t) ----
function ProxyCheck(apiKey){ this.apiKey = apiKey == null ? "" : "" + apiKey; this.includeVPN = true; this.useTLS = false; }
ProxyCheck.prototype.query = function(u, callback){
  var ip = (u && u.externalIp) ? u.externalIp : ("" + u);
  var url = (this.useTLS ? "https://" : "http://") + "proxycheck.io/v1/" + ip +
            (this.apiKey ? "&key=" + this.apiKey : "") + (this.includeVPN ? "&vpn=1" : "");
  var r = new HttpRequest();
  r.method = "POST"; r.src = url; r.utf = true; r.userAgent = "Astra";
  r.oncomplete = function(body, status, error){
    var result = null; try { result = JSON.parse(body); } catch (e) {}
    if (typeof callback === "function") callback(result, status, error);
  };
  return r.download();
};

// ---- XmlParser: DOM XML minimalista, implementación pura JS ----
function XmlNode(name){ this.nodeName = name || ""; this.nodeValue = ""; this.attributes = {}; this.childNodes = []; this.parentNode = null; }
XmlNode.prototype.appendChild = function(n){ n.parentNode = this; this.childNodes.push(n); return n; };
XmlNode.prototype.removeChild = function(n){ var i = this.childNodes.indexOf(n); if (i >= 0){ this.childNodes.splice(i, 1); n.parentNode = null; return true; } return false; };
XmlNode.prototype.getNodesByName = function(name){
  var out = [];
  (function walk(node){ for (var i = 0; i < node.childNodes.length; i++){ var c = node.childNodes[i]; if (c.nodeName === name) out.push(c); walk(c); } })(this);
  return out;
};
function XmlParser(){ this.available = false; this.root = null; this.xml = ""; }
XmlParser.prototype.create = function(rootName){ this.root = new XmlNode(rootName || "root"); this.available = true; return this.root; };
XmlParser.prototype.getNodesByName = function(name){ return this.root ? this.root.getNodesByName(name) : []; };
Object.defineProperty(XmlParser.prototype, "nodeName", { get: function(){ return this.root ? this.root.nodeName : ""; } });
Object.defineProperty(XmlParser.prototype, "nodeValue", { get: function(){ return this.root ? this.root.nodeValue : ""; } });
Object.defineProperty(XmlParser.prototype, "childNodes", { get: function(){ return this.root ? this.root.childNodes : []; } });
Object.defineProperty(XmlParser.prototype, "attributes", { get: function(){ return this.root ? this.root.attributes : {}; } });
Object.defineProperty(XmlParser.prototype, "parentNode", { get: function(){ return null; } });
XmlParser.prototype.load = function(xml){
  this.xml = xml == null ? "" : "" + xml;
  this.available = false; this.root = null;
  var s = this.xml, i = 0, n = s.length;
  var stack = [], root = null;
  function skipDecl(){ // saltar <?xml...?> y <!-- --> y <!DOCTYPE ...>
    while (i < n){
      if (s.slice(i, i + 2) === "<?"){ var e = s.indexOf("?>", i); if (e < 0) { i = n; return; } i = e + 2; }
      else if (s.slice(i, i + 4) === "<!--"){ var e2 = s.indexOf("-->", i); if (e2 < 0){ i = n; return; } i = e2 + 3; }
      else if (s.slice(i, i + 2) === "<!"){ var e3 = s.indexOf(">", i); if (e3 < 0){ i = n; return; } i = e3 + 1; }
      else break;
      while (i < n && /\s/.test(s[i])) i++;
    }
  }
  function unescape(t){ return t.replace(/&lt;/g,"<").replace(/&gt;/g,">").replace(/&quot;/g,'"').replace(/&apos;/g,"'").replace(/&amp;/g,"&"); }
  try {
    while (i < n){
      while (i < n && /\s/.test(s[i])) i++;
      if (i >= n) break;
      if (s.slice(i, i + 2) === "<?" || s.slice(i, i + 4) === "<!--" || s.slice(i, i + 2) === "<!"){ skipDecl(); continue; }
      if (s[i] === "<"){
        if (s[i + 1] === "/"){ // cierre
          var ce = s.indexOf(">", i); i = ce + 1;
          if (stack.length) stack.pop();
          continue;
        }
        var te = s.indexOf(">", i);
        if (te < 0) break;
        var selfClose = s[te - 1] === "/";
        var inner = s.substring(i + 1, selfClose ? te - 1 : te).trim();
        var sp = inner.search(/\s/);
        var tag = sp < 0 ? inner : inner.substring(0, sp);
        var node = new XmlNode(tag);
        if (sp >= 0){
          var attrStr = inner.substring(sp);
          var re = /([\w:.-]+)\s*=\s*"([^"]*)"/g, m;
          while ((m = re.exec(attrStr)) !== null){ node.attributes[m[1]] = unescape(m[2]); }
        }
        if (stack.length) stack[stack.length - 1].appendChild(node); else root = node;
        if (!selfClose) stack.push(node);
        i = te + 1;
      } else {
        var lt = s.indexOf("<", i);
        var text = (lt < 0 ? s.substring(i) : s.substring(i, lt));
        if (stack.length && text.trim().length) stack[stack.length - 1].nodeValue += unescape(text.trim());
        i = lt < 0 ? n : lt;
      }
    }
    this.root = root; this.available = root != null;
  } catch (e){ this.available = false; this.root = null; }
  return this.available;
};

// ---- Query: objeto de datos (sb0t: new Query("... {0} ...", p0, p1)) ----
function Query(sql){ this.__sql = (sql == null ? "" : "" + sql); this.__params = Array.prototype.slice.call(arguments, 1); }

// ---- Sql: DB SQLite propia del script (backend nativo) ----
function Sql(){ this.__h = __Sql_new(); }
Sql.prototype.open  = function(file){ return __Sql_open(this.__h, "" + file); };
Sql.prototype.query = function(q){ return __Sql_query(this.__h, q ? q.__sql : "", q ? q.__params : []); };
Sql.prototype.value = function(col){ return __Sql_value(this.__h, "" + col); };
Sql.prototype.close = function(){ return __Sql_close(this.__h); };
Object.defineProperty(Sql.prototype, "canRead",   { get: function(){ return __Sql_canRead(this.__h); } });
Object.defineProperty(Sql.prototype, "lastError", { get: function(){ return __Sql_lastError(this.__h); } });

// ---- PM: texto de un privado con helpers (JSPM). Se comporta como string. ----
function PM(text){ this.__t = text == null ? "" : "" + text; }
Object.defineProperty(PM.prototype, "isScribble", { get: function(){ return this.__t.indexOf('#scribble') === 0 || /^data:image/i.test(this.__t); } });
PM.prototype.contains = function(s){ return this.__t.indexOf(s) >= 0; };
PM.prototype.remove = function(s){ this.__t = this.__t.split(s).join(""); return this; };
PM.prototype.replace = function(a, b){ this.__t = this.__t.split(a).join(b); return this; };
PM.prototype.toString = function(){ return this.__t; };
PM.prototype.valueOf = function(){ return this.__t; };

// __mkPM: el mensaje que se pasa a onPMBefore. Es un String OBJECT (tiene
// TODOS los métodos de string nativos → compat con scripts que lo usan como
// string) MÁS los helpers JSPM de sb0t (contains/remove/replace/isScribble).
function __mkPM(text){
  var t = text == null ? "" : "" + text;
  var s = new String(t);
  s.contains = function(x){ return t.indexOf(x) >= 0; };
  s.remove = function(x){ return __mkPM(t.split(x).join("")); };
  s.replace = function(a, b){ return __mkPM(t.split(a).join(b)); };
  Object.defineProperty(s, "isScribble", { get: function(){ return t.indexOf('#scribble') === 0 || /^data:image/i.test(t); } });
  return s;
}

// ---- Leaf: hub linkeado (JSLeaf). Astra no linkea → stub con defaults seguros. ----
function Leaf(){ this.externalIp = ""; this.port = 0; this.name = ""; this.hashlink = ""; }
Leaf.prototype.print = function(){ return false; };
Leaf.prototype.printAdmins = function(){ return false; };
Leaf.prototype.users = function(){ return []; };
Leaf.prototype.user = function(){ return null; };
Leaf.prototype.sendText = function(){ return false; };
Leaf.prototype.sendEmote = function(){ return false; };
Leaf.prototype.scribble = function(){ return false; };
"#;

// ============================================================================
// Implementaciones de las native functions
// ============================================================================

/// Formatea un valor escalar como texto (para los mensajes de `print`).
/// bool → "true"/"false"; number → entero si aplica; string tal cual;
/// null/undefined/object → "".
fn format_scalar(v: &JsValue) -> String {
    if v.is_null() || v.is_undefined() {
        String::new()
    } else if let Some(b) = v.as_boolean() {
        b.to_string()
    } else if let Some(s) = v.as_string() {
        s.to_std_string_escaped()
    } else if let Some(n) = v.as_number() {
        if n.fract() == 0.0 && n.abs() < 1e15 {
            (n as i64).to_string()
        } else {
            n.to_string()
        }
    } else {
        String::new()
    }
}

/// Extrae el nombre (`__name`) de un objeto `user` pasado como argumento.
fn user_name_from_arg(v: &JsValue, ctx: &mut Context) -> Option<String> {
    let obj = v.as_object()?.clone();
    let name = obj.get(js_string!("__name"), ctx).ok()?;
    if name.is_undefined() {
        return None;
    }
    jsvalue_to_string(&name)
}

/// `print(...)` — paridad con sb0t (`JSGlobal.Print`):
/// - `print(texto)` → mensaje del bot a **toda la sala**.
/// - `print(vroom, texto)` → a los usuarios de ese vroom.
/// - `print(user, texto)` → mensaje del bot a **ese usuario**.
///
/// (Antes escribía a los logs del server, que NO es lo que hace sb0t. Para
/// logging desde scripts está `log()`.)
fn print_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let a = args.first().cloned();
    let b = args.get(1).cloned();
    let b_undefined = b.as_ref().map(|v| v.is_undefined()).unwrap_or(true);

    // print(texto) → a toda la sala.
    if b_undefined {
        let text = a.as_ref().map(format_scalar).unwrap_or_default();
        if !text.is_empty() {
            if let Some(app) = lookup_app(ctx) {
                let bot = app.settings.bot_name.clone();
                broadcast_to_users(&app, |c| {
                    server_core::outbound::build_public_c(&bot, &text, c)
                });
            }
        }
        return Ok(JsValue::undefined());
    }

    let text = b.as_ref().map(format_scalar).unwrap_or_default();

    // print(vroom, texto) → a un vroom (primer arg numérico).
    if let Some(vr) = a.as_ref().and_then(|v| v.as_number()) {
        if let Some(app) = lookup_app(ctx) {
            let bot = app.settings.bot_name.clone();
            let vr = vr as u16;
            for u in app.user_pool.users() {
                if u.logged_in && *u.vroom.read() == vr {
                    let _ = u.send_public(&bot, &text);
                }
            }
        }
        return Ok(JsValue::undefined());
    }

    // print(user, texto) → a ese usuario (primer arg = objeto user).
    if let Some(name) = a.as_ref().and_then(|v| user_name_from_arg(v, ctx)) {
        if let Some(app) = lookup_app(ctx) {
            let bot = app.settings.bot_name.clone();
            if let Some(target) = app.user_pool.get_by_name(&name) {
                let _ = target.send_public(&bot, &text);
            }
        }
    }
    Ok(JsValue::undefined())
}

fn log_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let msg = args.get(0).map(format_js_value).unwrap_or_default();
    tracing::info!("[script] log: {}", msg);
    Ok(JsValue::undefined())
}

fn send_public_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let from = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        broadcast_to_users(&app, |c| server_core::outbound::build_public_c(&from, &text, c));
        Ok(JsValue::from(true))
    } else {
        Ok(JsValue::from(false))
    }
}

fn send_emote_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let from = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        broadcast_to_users(&app, |c| server_core::outbound::build_emote_c(&from, &text, c));
        Ok(JsValue::from(true))
    } else {
        Ok(JsValue::from(false))
    }
}

fn send_pm_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let from = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let to = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(target) = app.user_pool.get_by_name(&to) {
            let _ = target.send_pvt(&from, &text);
            Ok(JsValue::from(true))
        } else {
            Ok(JsValue::from(false))
        }
    } else {
        Ok(JsValue::from(false))
    }
}

/// `__send_to_user(name, kind, sender, text)` — envía a UN usuario, para las
/// formas sb0t de `sendText`/`sendEmote`/`sendPM` que reciben un JSUser como
/// primer argumento. `kind` = "public" | "emote" | "pm".
fn send_to_user_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.first().and_then(jsvalue_to_string).unwrap_or_default();
    let kind = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let sender = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(3).and_then(jsvalue_to_string).unwrap_or_default();
    if sender.is_empty() || text.is_empty() {
        return Ok(JsValue::from(false));
    }
    if let Some(app) = lookup_app(ctx) {
        if let Some(target) = app.user_pool.get_by_name(&name) {
            let ok = match kind.as_str() {
                "emote" => {
                    let pkt = server_core::outbound::build_emote_c(&sender, &text, target.ares_crypto);
                    target.send(pkt)
                }
                "pm" => target.send_pvt(&sender, &text),
                _ => target.send_public(&sender, &text),
            };
            return Ok(JsValue::from(ok));
        }
    }
    Ok(JsValue::from(false))
}

fn user_count_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let n = lookup_app(ctx).map(|a| a.user_pool.len()).unwrap_or(0);
    Ok(JsValue::from(n as i32))
}

fn user_names_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    if let Some(app) = lookup_app(ctx) {
        let names: Vec<String> = app
            .user_pool
            .users()
            .into_iter()
            .filter(|u| u.logged_in)
            .map(|u| u.name.read().clone())
            .collect();
        let json = serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string());
        // Devolver el array JS de verdad (antes retornaba undefined).
        let arr = ctx
            .eval(boa_engine::Source::from_bytes(json.as_bytes()))
            .unwrap_or(JsValue::undefined());
        Ok(arr)
    } else {
        Ok(JsValue::undefined())
    }
}

fn user_exists_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        Ok(JsValue::from(app.user_pool.get_by_name(&name).is_some()))
    } else {
        Ok(JsValue::from(false))
    }
}

fn get_user_ip_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(u) = app.user_pool.get_by_name(&name) {
            Ok(JsValue::from(js_string!(u.external_ip.to_string())))
        } else {
            Ok(JsValue::null())
        }
    } else {
        Ok(JsValue::null())
    }
}

fn get_user_level_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(u) = app.user_pool.get_by_name(&name) {
            Ok(JsValue::from(*u.level.read() as i32))
        } else {
            Ok(JsValue::from(-1i32))
        }
    } else {
        Ok(JsValue::from(-1i32))
    }
}

fn get_user_vroom_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(u) = app.user_pool.get_by_name(&name) {
            Ok(JsValue::from(*u.vroom.read() as i32))
        } else {
            Ok(JsValue::from(-1i32))
        }
    } else {
        Ok(JsValue::from(-1i32))
    }
}

fn kick_user_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(u) = app.user_pool.get_by_name(&name) {
            // Kick best-effort: enviar ServerError, remover del pool. El
            // TCP handler verá el cierre del socket y limpiará.
            let mut w = proto_ares::PacketWriter::with_msg_crypto(
                proto_ares::TcpMsg::ServerError,
                u.ares_crypto,
            );
            w.write_string_nt("You have been kicked from the room.").ok();
            let _ = u.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
            let uid = u.id;
            app.user_pool.remove(uid);
            Ok(JsValue::from(true))
        } else {
            Ok(JsValue::from(false))
        }
    } else {
        Ok(JsValue::from(false))
    }
}

/// `__user_get(name, prop)` — devuelve una propiedad del usuario `name` para
/// el objeto `user`/JSUser del prelude. Null si el user no existe o la
/// propiedad no está soportada.
fn user_get_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    use std::sync::atomic::Ordering::Relaxed;
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let prop = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::null());
    };
    let Some(u) = app.user_pool.get_by_name(&name) else {
        return Ok(JsValue::null());
    };
    let v = match prop.as_str() {
        "name" => JsValue::from(js_string!(u.name.read().clone())),
        "orgName" => JsValue::from(js_string!(u.org_name.read().clone())),
        "id" => JsValue::from(u.id as f64),
        "level" => JsValue::from(*u.level.read() as u8 as f64),
        "vroom" => JsValue::from(*u.vroom.read() as f64),
        "externalIp" => JsValue::from(js_string!(u.external_ip.to_string())),
        "localIp" => JsValue::from(js_string!(u.local_ip.to_string())),
        "dns" => JsValue::from(js_string!(u.dns.read().clone())),
        "guid" => JsValue::from(js_string!(u
            .guid
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>())),
        "version" => JsValue::from(js_string!(u.version.clone())),
        "age" => JsValue::from(u.age as f64),
        "gender" | "sex" => JsValue::from(u.sex as f64),
        "country" => JsValue::from(u.country as f64),
        "region" => JsValue::from(js_string!(u.region.clone())),
        "fileCount" => JsValue::from(u.file_count as f64),
        "port" => JsValue::from(u.data_port as f64),
        "muzzled" => JsValue::from(u.is_muzzled()),
        "cloaked" => JsValue::from(u.cloaked.load(Relaxed)),
        "registered" => JsValue::from(u.registered),
        "encrypted" => JsValue::from(u.encrypted),
        "owner" => JsValue::from(*u.level.read() as u8 >= server_core::ILevel::Owner as u8),
        "webClient" => JsValue::from(u.web_client),
        "customClient" => JsValue::from(u.custom_client),
        "browsable" => JsValue::from(u.browsable),
        "fastPing" => JsValue::from(u.fast_ping),
        "canHTML" => JsValue::from(u.supports_html),
        "personalMessage" => JsValue::from(js_string!(u.personal_message.lock().clone())),
        "customName" => JsValue::from(js_string!(u.custom_name.read().clone().unwrap_or_default())),
        "joinTime" => JsValue::from(u.join_time as f64),
        // Props añadidas por la auditoría de scripting (paridad JSUser sb0t).
        "captcha" => JsValue::from(!u.needs_captcha.load(Relaxed)),
        "idle" => JsValue::from(app.idle.is_idle(u.id)),
        "visible" => JsValue::from(!u.cloaked.load(Relaxed)),
        "ghost" => JsValue::from(u.ghosting),
        "localEP" => JsValue::from(js_string!(format!("{}:{}", u.local_ip, u.data_port))),
        "linked" => JsValue::from(false),
        "leaf" => JsValue::null(),
        "fontJson" => {
            let f = &u.font;
            JsValue::from(js_string!(serde_json::json!({
                "name": f.face, "size": f.size, "color": f.color,
                "bold": f.bold, "italic": f.italic, "underline": f.underline,
            }).to_string()))
        }
        "ignoresJson" => {
            let list = u.ignore_list.read().clone();
            JsValue::from(js_string!(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())))
        }
        _ => JsValue::null(),
    };
    Ok(v)
}

/// `__banned_json()` — lista de bans activos como JSON (para `Users.banned()`).
fn banned_json_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let json = lookup_app(ctx)
        .map(|app| {
            let mut arr = Vec::new();
            app.bans.for_each(|b| {
                arr.push(serde_json::json!({
                    "name": b.name,
                    "version": b.version,
                    "externalIp": b.external_ip.to_string(),
                    "ident": b.ident,
                }));
            });
            serde_json::Value::Array(arr).to_string()
        })
        .unwrap_or_else(|| "[]".to_string());
    Ok(JsValue::from(js_string!(json)))
}

/// `__unban_ident(ident)` — quita un ban por ident (para `bannedUser.unban()`).
fn unban_ident_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let ident = args
        .get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    let ok = lookup_app(ctx).map(|app| app.bans.unban(ident)).unwrap_or(false);
    Ok(JsValue::from(ok))
}

/// `__user_do(name, action, arg)` — ejecuta una acción sobre el usuario
/// `name` (métodos del objeto `user`). Devuelve bool de éxito.
fn user_do_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let action = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let arg = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let Some(u) = app.user_pool.get_by_name(&name) else {
        return Ok(JsValue::from(false));
    };
    let bot = app.settings.bot_name.clone();
    let ok = match action.as_str() {
        "kick" | "disconnect" => {
            let mut w = proto_ares::PacketWriter::with_msg_crypto(
                proto_ares::TcpMsg::ServerError,
                u.ares_crypto,
            );
            w.write_string_nt("You have been kicked from the room.").ok();
            let _ = u.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
            // PART broadcast a toda la sala (paridad IUser.Disconnect de
            // sb0t) — sin esto los demás clientes ven un usuario fantasma.
            app.force_part_user(&u);
            true
        }
        "ban" => {
            let ident = app.bans.ban(
                &u.name.read(),
                &u.version,
                &u.guid,
                u.external_ip,
                u.local_ip,
                u.data_port,
            );
            if ident != 0 {
                // Registro para /banstats (el "banner" es el script).
                app.record_ban("script", &u.name.read(), &u.external_ip.to_string());
                let mut w = proto_ares::PacketWriter::with_msg_crypto(
                    proto_ares::TcpMsg::ServerError,
                    u.ares_crypto,
                );
                w.write_string_nt("You have been banned from the room.").ok();
                let _ = u.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
                app.force_part_user(&u);
                true
            } else {
                false
            }
        }
        // Paridad sb0t `IUser.SendText/SendEmote` (AresClient.cs:345): el
        // usuario "dice" el texto/emote EN PÚBLICO a todo su vroom — es el
        // mecanismo de #clone. (El PM del bot es `sendPM`.)
        "sendText" => {
            let name = u.name.read().clone();
            let vroom = *u.vroom.read();
            for other in app.user_pool.users() {
                if other.logged_in
                    && *other.vroom.read() == vroom
                    && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                    && !other.ignore_list.read().iter().any(|n| n.eq_ignore_ascii_case(&name))
                {
                    let _ = other.send_public(&name, &arg);
                }
            }
            true
        }
        "sendEmote" => {
            let name = u.name.read().clone();
            let vroom = *u.vroom.read();
            for other in app.user_pool.users() {
                if other.logged_in
                    && *other.vroom.read() == vroom
                    && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                    && !other.ignore_list.read().iter().any(|n| n.eq_ignore_ascii_case(&name))
                {
                    let _ = other.send_emote(&name, &arg);
                }
            }
            true
        }
        // sendHTML: a ESE usuario (los clientes Astra no renderizan HTML,
        // va como texto de sistema — paridad funcional con SendHTML).
        "sendPM" | "sendHTML" => u.send_pvt(&bot, &arg),
        // ---- Setters writable (paridad JSUser de sb0t) ----
        "set:customName" => {
            let next = if arg.trim().is_empty() {
                None
            } else {
                Some(arg.trim().chars().take(40).collect::<String>())
            };
            *u.custom_name.write() = next.clone();
            app.publish_link_event(server_core::LinkEvent::CustomName {
                origin: None,
                name: u.name.read().clone(),
                custom_name: next,
            });
            true
        }
        "set:muzzled" => {
            let v = arg == "true" || arg == "1";
            u.muzzled.store(v, std::sync::atomic::Ordering::Relaxed);
            app.publish_link_event(server_core::LinkEvent::UserUpdated {
                origin: None,
                user: server_core::LinkUserSnapshot::from_user(&u),
            });
            true
        }
        "set:vroom" => {
            let Ok(new_vroom) = arg.trim().parse::<u16>() else {
                return Ok(JsValue::from(false));
            };
            let old_vroom = *u.vroom.read();
            if old_vroom == new_vroom {
                return Ok(JsValue::from(true));
            }
            // PART del vroom viejo + JOIN al nuevo para los espectadores.
            let mut part_user = server_core::user_pool::AresUser::new(u.id, u.external_ip, u.guid);
            part_user.logged_in = true;
            *part_user.name.write() = u.name.read().clone();
            *part_user.vroom.write() = old_vroom;
            *u.vroom.write() = new_vroom;
            for other in app.user_pool.users() {
                if !other.logged_in
                    || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                {
                    continue;
                }
                let ov = *other.vroom.read();
                if ov == old_vroom {
                    let _ = other.send(server_core::outbound::build_part_c(&part_user, other.ares_crypto));
                }
                if ov == new_vroom {
                    let _ = other.send(server_core::outbound::build_join_or_userlist_c(&u, other.ares_crypto));
                }
            }
            app.publish_link_event(server_core::LinkEvent::VroomChanged {
                origin: None,
                user: server_core::LinkUserSnapshot::from_user(&u),
            });
            true
        }
        "set:level" => {
            let Ok(v) = arg.trim().parse::<u8>() else {
                return Ok(JsValue::from(false));
            };
            let new_level = match v {
                0 => server_core::ILevel::Anonymous,
                1 => server_core::ILevel::Regular,
                2 => server_core::ILevel::Voice,
                50 => server_core::ILevel::Moderator,
                80 => server_core::ILevel::Admin,
                100 => server_core::ILevel::Owner,
                _ => return Ok(JsValue::from(false)),
            };
            *u.level.write() = new_level;
            if let Ok(Some(_)) = app.accounts.find_by_guid(&u.guid) {
                let _ = app.accounts.set_level(&u.guid, new_level as u8);
            }
            // Refrescar el nivel en los clientes de su vroom.
            let vroom = *u.vroom.read();
            let uname = u.name.read().clone();
            let lvl_str = (new_level as u8).to_string();
            let ws_msg = format!("UPDATE:{},{}:{}{}", uname.encode_utf16().count(), lvl_str.len(), uname, lvl_str);
            for other in app.user_pool.users() {
                if !other.logged_in || *other.vroom.read() != vroom {
                    continue;
                }
                if let Some(tx) = &other.ws_text_sender {
                    let _ = tx.send(ws_msg.clone());
                } else {
                    let _ = other.send(server_core::outbound::build_join_or_userlist_c(&u, other.ares_crypto));
                }
            }
            app.publish_link_event(server_core::LinkEvent::UserUpdated {
                origin: None,
                user: server_core::LinkUserSnapshot::from_user(&u),
            });
            true
        }
        "set:avatar" => {
            if arg.trim().is_empty() {
                *u.avatar.lock() = None;
                true
            } else {
                match base64_decode_bytes(arg.trim()) {
                    Some(bytes) => {
                        *u.avatar.lock() = Some(bytes);
                        true
                    }
                    None => false,
                }
            }
        }
        // ---- Métodos restantes ----
        "redirect" => match server_core::hashlink::decode(&arg) {
            Some(hr) => {
                let _ = u.send(server_core::outbound::build_redirect_c(
                    std::net::IpAddr::V4(hr.ip),
                    hr.port,
                    &app.settings.room_name,
                    u.ares_crypto,
                ));
                app.force_part_user(&u);
                true
            }
            None => {
                // También ip:port plano (extra de Astra).
                if let Some((ip_s, port_s)) = arg.rsplit_once(':') {
                    if let (Ok(ip), Ok(port)) = (ip_s.parse::<std::net::IpAddr>(), port_s.parse::<u16>()) {
                        let _ = u.send(server_core::outbound::build_redirect_c(
                            ip, port, &app.settings.room_name, u.ares_crypto,
                        ));
                        app.force_part_user(&u);
                        return Ok(JsValue::from(true));
                    }
                }
                false
            }
        },
        "setTopic" => {
            // Topic dirigido SOLO a ese usuario (paridad IUser.SetTopic).
            let _ = u.send(server_core::outbound::build_topic_c(&arg, u.ares_crypto));
            true
        }
        "nudge" => {
            // Sin opcode de nudge dedicado: se aproxima con un texto del
            // server a ese usuario (los clientes web no soportan buzz).
            u.print(&bot, "*** nudge ***")
        }
        _ => false,
    };
    Ok(JsValue::from(ok))
}

/// `__room_get(prop)` — propiedades de la sala (objeto `Room`).
fn room_get_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let prop = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::null());
    };
    let v = match prop.as_str() {
        "name" => JsValue::from(js_string!(app.settings.room_name.clone())),
        "botName" => JsValue::from(js_string!(app.settings.bot_name.clone())),
        "topic" => JsValue::from(js_string!(app.current_room_topic())),
        "port" => JsValue::from(app.settings.port as f64),
        "version" => JsValue::from(js_string!(env!("CARGO_PKG_VERSION"))),
        "externalIp" => JsValue::from(js_string!("")),
        "startTime" => JsValue::from(app.uptime_secs() as f64),
        _ => JsValue::null(),
    };
    Ok(v)
}

/// `__stats_get(name)` — contadores del objeto `Stats`. 0 para los que Astra
/// aún no rastrea.
fn stats_get_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(0.0));
    };
    let v: f64 = match name.as_str() {
        "userCount" => app.user_pool.len() as f64,
        "peakUserCount" => app.stats.peak_users() as f64,
        "joinCount" => app.stats.total_users() as f64,
        "dataReceived" => app.stats.bytes_in() as f64,
        "dataSent" => app.stats.bytes_out() as f64,
        "messageCount" => app.stats.messages() as f64,
        "pmCount" => app.stats.pms() as f64,
        "floodCount" => app.stats.floods() as f64,
        "partCount" => app.stats.parts() as f64,
        "invalidLoginCount" => app.stats.invalid_logins() as f64,
        "rejectionCount" => app.stats.rejections() as f64,
        _ => 0.0,
    };
    Ok(JsValue::from(v))
}

/// `__records_json()` — historial de usuarios desconectados como JSON array
/// (para `Users.records()`). Cada entrada trae los campos + el guid en hex.
fn records_json_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(js_string!("[]")));
    };
    let recs = app.user_records.read();
    let mut items: Vec<String> = Vec::with_capacity(recs.len());
    for r in recs.iter() {
        let guid_hex: String = r.guid.iter().map(|b| format!("{:02x}", b)).collect();
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        items.push(format!(
            "{{\"name\":\"{}\",\"externalIp\":\"{}\",\"localIp\":\"{}\",\"version\":\"{}\",\"port\":{},\"guid\":\"{}\",\"dns\":\"{}\",\"joinTime\":{}}}",
            esc(&r.name),
            r.external_ip,
            r.local_ip,
            esc(&r.version),
            r.port,
            guid_hex,
            esc(&r.dns),
            r.join_time
        ));
    }
    Ok(JsValue::from(js_string!(format!("[{}]", items.join(",")))))
}

/// `__record_ban(name, version, guidHex, externalIp, localIp, port)` — banea
/// a partir de los datos de un record histórico. Devuelve bool.
fn record_ban_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let version = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let guid_hex = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    let ext_ip = args.get(3).and_then(jsvalue_to_string).unwrap_or_default();
    let local_ip = args.get(4).and_then(jsvalue_to_string).unwrap_or_default();
    let port = args.get(5).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    // parsear guid hex → [u8;16]
    let mut guid = [0u8; 16];
    if guid_hex.len() == 32 {
        for i in 0..16 {
            match u8::from_str_radix(&guid_hex[i * 2..i * 2 + 2], 16) {
                Ok(b) => guid[i] = b,
                Err(_) => return Ok(JsValue::from(false)),
            }
        }
    }
    let (Ok(ext), Ok(loc)) = (ext_ip.parse(), local_ip.parse()) else {
        return Ok(JsValue::from(false));
    };
    let ident = app.bans.ban(&name, &version, &guid, ext, loc, port);
    Ok(JsValue::from(ident != 0))
}

fn get_topic_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let topic = lookup_app(ctx).map(|a| a.current_room_topic()).unwrap_or_default();
    Ok(JsValue::from(js_string!(topic)))
}

fn set_topic_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let topic = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        app.set_room_topic(topic.clone());
        broadcast_to_users(&app, |c| server_core::outbound::build_topic_c(&topic, c));
        Ok(JsValue::from(true))
    } else {
        Ok(JsValue::from(false))
    }
}

fn hash_sha1_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(s.as_bytes());
    let result = h.finalize();
    let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(JsValue::from(js_string!(hex)))
}

fn hash_md5_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    let result = h.finalize();
    let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(JsValue::from(js_string!(hex)))
}

fn b64_enc_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    Ok(JsValue::from(js_string!(base64_encode(&s))))
}

fn b64_dec_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match base64_decode(&s) {
        Some(d) => Ok(JsValue::from(js_string!(d))),
        None => Ok(JsValue::null()),
    }
}

// ============ File I/O ============

fn file_exists_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let exists = resolve_script_path(ctx, &arg, false)
        .map(|p| p.exists())
        .unwrap_or(false);
    Ok(JsValue::from(exists))
}

fn file_size_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let size = resolve_script_path(ctx, &arg, false)
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as i64)
        .unwrap_or(-1);
    Ok(JsValue::from(size))
}

fn file_creation_time_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let secs = resolve_script_path(ctx, &arg, false)
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.created().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(JsValue::from(secs))
}

/// `File_read(name)` → contenido del archivo (relativo a la carpeta del
/// script), o `null` si no existe. Para datos del script.
fn file_read_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match resolve_script_path(ctx, &arg, false).and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(s) => Ok(JsValue::from(js_string!(s))),
        None => Ok(JsValue::null()),
    }
}

/// `__read_file_b64(name)` → lee el archivo como bytes crudos y devuelve su
/// base64 (para cargar imágenes binarias en Avatar/Scribble). Null si falla.
fn read_file_b64_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match resolve_script_path(ctx, &arg, false).and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => Ok(JsValue::from(js_string!(base64_encode_bytes_to_string(&bytes)))),
        None => Ok(JsValue::null()),
    }
}

/// `File_write(name, text)` → escribe (sobrescribe) el archivo en la carpeta
/// del script. Retorna true/false.
fn file_write_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let ok = match resolve_script_path(ctx, &arg, false) {
        Some(p) => std::fs::write(p, text.as_bytes()).is_ok(),
        None => false,
    };
    Ok(JsValue::from(ok))
}

/// `File_append(name, text)` → agrega al final del archivo.
fn file_append_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    use std::io::Write as _;
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let ok = match resolve_script_path(ctx, &arg, false) {
        Some(p) => std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .is_ok(),
        None => false,
    };
    Ok(JsValue::from(ok))
}

/// `File_delete(name)` → borra el archivo de la carpeta del script.
fn file_delete_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let ok = match resolve_script_path(ctx, &arg, false) {
        Some(p) => std::fs::remove_file(p).is_ok(),
        None => false,
    };
    Ok(JsValue::from(ok))
}

// ============ Zip ============

fn zip_compress_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let data = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match zip_compress(data.as_bytes()) {
        Ok(bytes) => {
            // Retornar como base64 para que sea representable en JS (UTF-8 safe)
            Ok(JsValue::from(js_string!(base64_encode_bytes_to_string(&bytes))))
        }
        Err(e) => {
            tracing::warn!("Zip_compress error: {}", e);
            Ok(JsValue::null())
        }
    }
}

fn zip_decompress_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    // Decodificar base64 → bytes del zip
    let raw = match base64_decode_bytes(&s) {
        Some(bytes) => bytes,
        None => s.into_bytes(),
    };
    match zip_decompress(&raw) {
        Ok(bytes) => {
            match String::from_utf8(bytes) {
                Ok(text) => Ok(JsValue::from(js_string!(text))),
                Err(_) => Ok(JsValue::null()),
            }
        }
        Err(e) => {
            tracing::warn!("Zip_decompress error: {}", e);
            Ok(JsValue::null())
        }
    }
}

/// Decodifica base64 a bytes crudos (no asume UTF-8).
/// Usado por zip_decompress donde los datos son binarios.
fn base64_decode_bytes(s: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            buf[i] = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => 0,
                _ => return None,
            };
        }
        let n = chunk.iter().filter(|&&c| c != b'=').count();
        let combined = ((buf[0] as u32) << 18)
            | ((buf[1] as u32) << 12)
            | ((buf[2] as u32) << 6)
            | (buf[3] as u32);
        if n >= 2 { out.push((combined >> 16) as u8); }
        if n >= 3 { out.push((combined >> 8) as u8); }
        if n >= 4 { out.push(combined as u8); }
    }
    Some(out)
}

// helper: encode bytes a string base64 (UTF-8 safe)
fn base64_encode_bytes_to_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(B64[(b0 >> 2) as usize] as char);
        out.push(B64[((b0 << 4 | b1 >> 4) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64[((b1 << 2 | b2 >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn zip_compress(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("data.txt", options)
            .map_err(|e| format!("start_file: {}", e))?;
        zip.write_all(data).map_err(|e| format!("write: {}", e))?;
        zip.finish().map_err(|e| format!("finish: {}", e))?;
    }
    Ok(buf)
}

fn zip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("open zip: {}", e))?;
    let mut file = zip.by_index(0).map_err(|e| format!("read entry: {}", e))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| format!("read: {}", e))?;
    Ok(buf)
}

// ============ Script include ============

/// `includeAll()` — carga todos los `.js` de la carpeta del script EXCEPTO el
/// archivo principal (`<carpeta>.js`). Paridad sb0t (`includeAll`). Retorna la
/// cantidad de sub-scripts cargados.
fn include_all_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let Some(dir) = current_script_dir(ctx) else {
        return Ok(JsValue::from(0));
    };
    let main_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|n| format!("{}.js", n));
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("js"))
            .collect(),
        Err(_) => return Ok(JsValue::from(0)),
    };
    files.sort();
    let mut count = 0i64;
    for p in files {
        let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if Some(fname) == main_name.as_deref() {
            continue; // no re-evaluar el principal
        }
        if let Ok(source) = std::fs::read_to_string(&p) {
            match ctx.eval(boa_engine::Source::from_bytes(source.as_bytes())) {
                Ok(_) => count += 1,
                Err(e) => tracing::warn!("includeAll: error en {}: {}", p.display(), e),
            }
        }
    }
    Ok(JsValue::from(count))
}

fn script_include_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let arg = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    // Resolver relativo a la carpeta del script (paridad sb0t: `include(name)`
    // carga `<carpeta_del_script>/<name>.js`).
    let Some(path) = resolve_script_path(ctx, &arg, true) else {
        tracing::warn!("ScriptInclude: ruta inválida '{}'", arg);
        return Ok(JsValue::from(false));
    };
    let path = path.display().to_string();
    match std::fs::read_to_string(&path) {
        Ok(source) => {
            // Evaluar el script en el MISMO context. Las funciones/constantes
            // definidas en el script incluido quedan disponibles para el caller.
            match ctx.eval(boa_engine::Source::from_bytes(source.as_bytes())) {
                Ok(_) => {
                    tracing::info!("ScriptInclude: cargado {}", path);
                    Ok(JsValue::from(true))
                }
                Err(e) => {
                    tracing::warn!("ScriptInclude: error en {}: {}", path, e);
                    Ok(JsValue::from(false))
                }
            }
        }
        Err(e) => {
            tracing::warn!("ScriptInclude: no se pudo leer {}: {}", path, e);
            Ok(JsValue::from(false))
        }
    }
}

// ============ Spell check ============

/// Wordlist minimal (~100 palabras comunes en inglés) para spell check.
/// En el futuro podría leerse de un archivo de diccionario o usar `aspell`.
const SPELL_DICT: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i",
    "it", "for", "not", "on", "with", "he", "as", "you", "do", "at",
    "this", "but", "his", "by", "from", "they", "we", "say", "her", "she",
    "or", "an", "will", "my", "one", "all", "would", "there", "their", "what",
    "hello", "world", "yes", "no", "ok", "okay", "thanks", "please", "welcome", "bye",
    "good", "bad", "great", "nice", "cool", "fine", "test", "chat", "room", "user",
    "admin", "owner", "mod", "moderator", "op", "voice", "ban", "kick", "mute", "unban",
    "topic", "message", "public", "private", "pm", "join", "part", "leave", "enter", "exit",
    "help", "list", "info", "version", "status", "online", "offline", "away", "busy", "back",
    "happy", "sad", "angry", "love", "hate", "lol", "rofl", "haha", "hehe", "wtf",
    "mom", "dad", "bro", "sis", "friend", "buddy", "pal", "dude", "man", "woman",
];

fn spelling_check_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let word = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let word = word.trim();
    if word.is_empty() {
        return Ok(JsValue::from(false));
    }
    // Verificar que solo tenga letras
    if !word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-') {
        return Ok(JsValue::from(false));
    }
    // Verificar contra el diccionario (case-insensitive) o palabras
    // confirmadas en runtime vía Spelling.confirm.
    let lower = word.to_lowercase();
    let known = SPELL_DICT.iter().any(|w| w.eq_ignore_ascii_case(&lower))
        || CONFIRMED_WORDS.lock().unwrap().iter().any(|w| w == &lower);
    Ok(JsValue::from(known))
}

/// Palabras agregadas al diccionario en runtime vía `Spelling.confirm`.
static CONFIRMED_WORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `Spelling.confirm(word)` — agrega una palabra al diccionario en runtime
/// (paridad sb0t). Devuelve true.
fn spelling_confirm_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let word = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let word = word.trim().to_lowercase();
    if !word.is_empty() {
        let mut w = CONFIRMED_WORDS.lock().unwrap();
        if !w.iter().any(|x| x == &word) {
            w.push(word);
        }
    }
    Ok(JsValue::from(true))
}

/// `Spelling_suggest(word)` — devuelve JSON array de sugerencias de spell
/// para `word` (palabras del diccionario que comparten prefijo de 2+ chars).
/// Si `word` está en el diccionario, retorna `[]`.
fn spelling_suggest_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let word = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let word = word.trim().to_lowercase();
    if word.len() < 2 {
        return Ok(JsValue::from(js_string!("[]")));
    }
    let prefix = &word[..2];
    let mut suggestions: Vec<&str> = SPELL_DICT
        .iter()
        .copied()
        .filter(|w| w.to_lowercase().starts_with(prefix) && !w.eq_ignore_ascii_case(&word))
        .take(10)
        .collect();
    suggestions.sort();
    let json: Vec<String> = suggestions.iter().map(|s| format!("\"{}\"", s)).collect();
    Ok(JsValue::from(js_string!(format!("[{}]", json.join(",")))))
}

// ============ sb0t-compat: stubs honestos ============

/// Almacén thread-local de stats key→value (sb0t Stats_addStat/getStat).
/// Vive solo en el thread del ScriptManager; se pierde al recargar.
thread_local! {
    static STATS_STORE: std::cell::RefCell<std::collections::HashMap<String, i64>>
        = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Almacén thread-local del virtual HKLM registry.
thread_local! {
    static REGISTRY_STORE: std::cell::RefCell<std::collections::HashSet<String>>
        = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Almacén GLOBAL de líneas extra para /help (Fase 20).
///
/// OJO: debe ser global, no thread-local. Los scripts llaman `Help_addLine`
/// en el hilo dedicado de scripting, pero `extra_help_lines()` lo lee
/// `handle_help` desde un hilo de tokio (el que procesa el comando). Con un
/// thread-local, el lector veía un almacén vacío y las líneas de ayuda de los
/// scripts nunca aparecían en `/help` (bug reportado en producción).
static HELP_LINES: std::sync::Mutex<Vec<(String, String, String)>> = std::sync::Mutex::new(Vec::new());

/// Contador monotónico de timer IDs (Fase 20).
thread_local! {
    static TIMER_COUNTER: std::cell::RefCell<i32> = std::cell::RefCell::new(0);
}

/// Almacén de timer IDs activos (Fase 20). Para cancelación explícita.
thread_local! {
    static ACTIVE_TIMERS: std::cell::RefCell<std::collections::HashSet<i32>>
        = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// `Channels_list()` — devuelve los vroom IDs activos como JSON array.
/// Siempre incluye vroom 0 (main room) más los que se hayan creado.
fn channels_list_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let ids_json = lookup_app(ctx)
        .map(|a| a.vrooms.list_ids_json())
        .unwrap_or_else(|| "[0]".to_string());
    Ok(JsValue::from(js_string!(ids_json)))
}

/// `Channels_get(id)` — devuelve info del vroom como JSON string, o `null` si no existe.
/// Formato: `{"id":0,"name":"Main Room","topic":"..."}`
fn channels_get_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    let json = lookup_app(ctx)
        .map(|a| a.vrooms.get_json(id))
        .unwrap_or_else(|| "null".to_string());
    Ok(JsValue::from(js_string!(json)))
}

/// `Channels_create(id, name)` — crea un vroom nuevo. Retorna `true` si OK,
/// `false` si el ID ya existe.
fn channels_create_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16);
    let name = args.get(1).and_then(jsvalue_to_string);
    let Some(id) = id else {
        return Ok(JsValue::from(false));
    };
    let result = lookup_app(ctx)
        .map(|a| a.vrooms.create(id, name, None))
        .unwrap_or(false);
    Ok(JsValue::from(result))
}

/// `Channels_setTopic(id, topic)` — cambia el topic de un vroom existente.
/// Retorna `true` si OK, `false` si el vroom no existe.
fn channels_set_topic_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    let topic = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let result = lookup_app(ctx)
        .map(|a| a.vrooms.set_topic(id, topic))
        .unwrap_or(false);
    Ok(JsValue::from(result))
}

/// `Channels_broadcast(id, from, text)` — envía un mensaje público solo a
/// los users en el vroom `id`. Retorna `true` si se envió a alguien.
fn channels_broadcast_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16);
    let from = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let text = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(id) = id else {
        return Ok(JsValue::from(false));
    };
    if let Some(app) = lookup_app(ctx) {
        let mut sent = 0;
        for u in app.user_pool.users() {
            if !u.logged_in { continue; }
            if *u.vroom.read() != id { continue; }
            if u.quarantined.load(std::sync::atomic::Ordering::Relaxed) { continue; }
            if u.send(server_core::outbound::build_public_c(&from, &text, u.ares_crypto)) {
                sent += 1;
            }
        }
        Ok(JsValue::from(sent > 0))
    } else {
        Ok(JsValue::from(false))
    }
}

/// `Channels_kick(vroom_id, name)` — kickea a un user de un vroom.
/// Retorna `true` si el user estaba en ese vroom y se kickeó.
fn channels_kick_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let vroom_id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16);
    let name = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(vroom_id) = vroom_id else {
        return Ok(JsValue::from(false));
    };
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let Some(target) = app.user_pool.get_by_name(&name) else {
        return Ok(JsValue::from(false));
    };
    if *target.vroom.read() != vroom_id {
        return Ok(JsValue::from(false));
    }
    // Mover al vroom 0 (default)
    *target.vroom.write() = 0;
    // Notificar al user
    use proto_ares::TcpMsg;
    let mut w = proto_ares::PacketWriter::with_msg_crypto(TcpMsg::ServerError, target.ares_crypto);
    w.write_string_nt(&format!("You have been kicked from vroom {}.", vroom_id)).ok();
    let _ = target.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
    Ok(JsValue::from(true))
}

/// `Channels_delete(id)` — elimina un vroom. Los users en ese vroom
/// son movidos a vroom 0. Retorna `true` si se eliminó.
fn channels_delete_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    // Mover users de ese vroom a vroom 0
    let mut moved = 0;
    for u in app.user_pool.users() {
        if *u.vroom.read() == id {
            *u.vroom.write() = 0;
            moved += 1;
        }
    }
    // Eliminar el vroom
    let deleted = app.vrooms.delete(id);
    if moved > 0 {
        tracing::info!("Channels_delete({}): moved {} users to vroom 0", id, moved);
    }
    Ok(JsValue::from(deleted))
}

/// `Hashlink_create(server, port)` — genera URL hashlink.
/// Formato: `astrahash://server:port`
fn hashlink_create_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let server = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let port = args.get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    Ok(JsValue::from(js_string!(format!("astrahash://{}:{}", server, port))))
}

/// `Hashlink_parse(url)` — extrae server y port de un hashlink.
/// Retorna `{"server":"x.com","port":5009}` o `null` si el formato es inválido.
fn hashlink_parse_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let url = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    // Formato esperado: "astrahash://server:port"
    let rest = url.strip_prefix("astrahash://").unwrap_or(&url);
    // Split por último ':' (por si el server tiene IPv6 brackets)
    let (server, port_str) = match rest.rsplit_once(':') {
        Some((s, p)) => (s, p),
        None => return Ok(JsValue::null()),
    };
    // Quitar brackets IPv6 si los hay
    let server = server
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(server)
        .to_string();
    let port: u16 = match port_str.parse() {
        Ok(p) => p,
        Err(_) => return Ok(JsValue::null()),
    };
    if server.is_empty() {
        return Ok(JsValue::null());
    }
    let json = format!(
        "{{\"server\":\"{}\",\"port\":{}}}",
        server.replace('\\', "\\\\").replace('"', "\\\""),
        port
    );
    Ok(JsValue::from(js_string!(json)))
}

/// `Link_list()` — devuelve JSON array con los links activos.
/// Formato: `["name1:5009", "name2:5010"]`
fn link_list_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let json = lookup_app(ctx)
        .map(|a| {
            let links = a.link_servers.read();
            let entries: Vec<String> = links
                .iter()
                .map(|(name, port, _)| format!("\"{}:{}\"", name, port))
                .collect();
            format!("[{}]", entries.join(","))
        })
        .unwrap_or_else(|| "[]".to_string());
    Ok(JsValue::from(js_string!(json)))
}

/// `Link_getUserList()` — devuelve JSON array con todos los users locales + remotos.
fn link_get_user_list_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    if let Some(app) = lookup_app(ctx) {
        let mut names: Vec<String> = app
            .user_pool
            .users()
            .into_iter()
            .filter(|u| u.logged_in)
            .map(|u| u.name.read().clone())
            .collect();
        // Agregar users de leaves remotos
        for (_link, user) in app.link_users.read().iter() {
            if !names.contains(user) {
                names.push(user.clone());
            }
        }
        names.sort();
        let json: Vec<String> = names.iter().map(|n| format!("\"{}\"", n)).collect();
        return Ok(JsValue::from(js_string!(format!("[{}]", json.join(",")))));
    }
    Ok(JsValue::from(js_string!("[]")))
}

/// `Link_findLeaf(name)` — busca un leaf por nombre. Retorna `"name:port"` o `null`.
fn link_find_leaf_fn_real(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::null());
    };
    let links = app.link_servers.read();
    for (n, port, _) in links.iter() {
        if n == &name {
            return Ok(JsValue::from(js_string!(format!("{}:{}", n, port))));
        }
    }
    Ok(JsValue::null())
}

/// `Link_findUser(name)` — busca un user en cualquier leaf. Retorna `link:user` o `null`.
fn link_find_user_fn_real(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::null());
    };
    let users = app.link_users.read();
    for (link, user) in users.iter() {
        if user == &name {
            return Ok(JsValue::from(js_string!(format!("{}:{}", link, user))));
        }
    }
    Ok(JsValue::null())
}

/// `Link_findHub(name)` — busca un hub. Mismo set que findLeaf en esta impl.
fn link_find_hub_fn_real(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    link_find_leaf_fn_real(_this, args, ctx)
}

/// ⚠️ `Users_getUserByName(name)` — devuelve un string con info del user
/// o `null` si no existe. En el futuro debería devolver un objeto User real.
fn users_get_by_name_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        if let Some(u) = app.user_pool.get_by_name(&name) {
            // Formato compacto: "User:name:ip:level"
            let info = format!(
                "User:{}:{}:{}",
                u.name.read(),
                u.external_ip,
                *u.level.read() as u8,
            );
            return Ok(JsValue::from(js_string!(info)));
        }
    }
    Ok(JsValue::null())
}

/// `Stats_addStat(key, value)` — guarda un stat en memoria (thread-local).
fn stats_add_stat_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let key = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let value = args.get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as i64)
        .unwrap_or(0);
    STATS_STORE.with(|s| s.borrow_mut().insert(key, value));
    Ok(JsValue::from(true))
}

/// `Stats_getStat(key)` — lee un stat guardado por `Stats_addStat`. Retorna 0 si no existe.
fn stats_get_stat_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let key = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let value = STATS_STORE.with(|s| s.borrow().get(&key).copied()).unwrap_or(0);
    Ok(JsValue::from(value))
}

/// `Entities_list()` — devuelve la lista de nodos UDP conocidos como JSON array.
/// Cada elemento es `{"name":"x.com","port":5009,"users":42}`.
fn entities_list_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let json = lookup_app(ctx)
        .map(|a| {
            let nodes = a.udp_nodes.read();
            let entries: Vec<String> = nodes
                .iter()
                .map(|(name, port, users)| {
                    format!(
                        "{{\"name\":\"{}\",\"port\":{},\"users\":{}}}",
                        name.replace('\\', "\\\\").replace('"', "\\\""),
                        port,
                        users
                    )
                })
                .collect();
            format!("[{}]", entries.join(","))
        })
        .unwrap_or_else(|| "[]".to_string());
    Ok(JsValue::from(js_string!(json)))
}

/// `Link_createLink(name, server, port)` — crea una conexión link a otro server.
/// Envía un `LinkRequest::CreateLink` al bus. Retorna `true` si se encoló.
fn link_create_link_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let server = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let port = args.get(2)
        .and_then(|v| v.as_number())
        .map(|n| n as u16)
        .unwrap_or(0);
    if name.is_empty() || server.is_empty() || port == 0 {
        return Ok(JsValue::from(false));
    }
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let req = server_core::LinkRequest::CreateLink { name, server, port };
    Ok(JsValue::from(app.link_requests.send(req).is_ok()))
}

/// `Link_disconnect(name)` — desconecta el link con ese nombre.
/// Envía un `LinkRequest::DisconnectLink` al bus. Retorna `true` si se encoló.
fn link_disconnect_fn_real(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if name.is_empty() {
        return Ok(JsValue::from(false));
    }
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let req = server_core::LinkRequest::DisconnectLink { name };
    Ok(JsValue::from(app.link_requests.send(req).is_ok()))
}

/// `Link_kickHub(name)` — fuerza la desconexión de un hub.
/// Envía un `LinkRequest::KickHub` al bus. Retorna `true` si se encoló.
fn link_kick_hub_fn_real(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if name.is_empty() {
        return Ok(JsValue::from(false));
    }
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let req = server_core::LinkRequest::KickHub { name };
    Ok(JsValue::from(app.link_requests.send(req).is_ok()))
}

/// `Registry_createKey(name)` — virtual HKLM. Crea una "key" en memoria
/// thread-local. Retorna la ruta completa: `HKLM\Software\Astra\{name}`.
fn registry_create_key_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let path = format!("HKLM\\Software\\Astra\\{}", name);
    REGISTRY_STORE.with(|r| r.borrow_mut().insert(path.clone()));
    Ok(JsValue::from(js_string!(path)))
}

/// `Registry_deleteKey(name)` — virtual HKLM. Borra una key.
fn registry_delete_key_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let path = format!("HKLM\\Software\\Astra\\{}", name);
    let removed = REGISTRY_STORE.with(|r| r.borrow_mut().remove(&path));
    Ok(JsValue::from(removed))
}

// ============ Registry persistente (KV por script, compat sb0t) ============
//
// sb0t expone `Registry` como un almacén clave→valor persistente por script.
// Astra lo respalda en `<script_dir>/registry.json`. Si no hay script dir
// (p.ej. tests), cae a un mapa en memoria thread-local.

thread_local! {
    static REGISTRY_MEM: RefCell<serde_json::Map<String, serde_json::Value>>
        = RefCell::new(serde_json::Map::new());
}

fn registry_file(ctx: &mut Context) -> Option<std::path::PathBuf> {
    current_script_dir(ctx).map(|d| d.join("registry.json"))
}

fn registry_load(ctx: &mut Context) -> serde_json::Map<String, serde_json::Value> {
    if let Some(path) = registry_file(ctx) {
        if let Ok(txt) = std::fs::read_to_string(&path) {
            if let Ok(serde_json::Value::Object(m)) = serde_json::from_str(&txt) {
                return m;
            }
        }
        serde_json::Map::new()
    } else {
        REGISTRY_MEM.with(|m| m.borrow().clone())
    }
}

fn registry_store(ctx: &mut Context, map: &serde_json::Map<String, serde_json::Value>) -> bool {
    if let Some(path) = registry_file(ctx) {
        match serde_json::to_string(map) {
            Ok(txt) => std::fs::write(&path, txt).is_ok(),
            Err(_) => false,
        }
    } else {
        REGISTRY_MEM.with(|m| *m.borrow_mut() = map.clone());
        true
    }
}

/// `Registry.getValue(name)` — valor asociado, o null si no existe.
fn registry_get_value_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let map = registry_load(ctx);
    match map.get(&name).and_then(|v| v.as_str()) {
        Some(s) => Ok(JsValue::from(js_string!(s.to_string()))),
        None => Ok(JsValue::null()),
    }
}

/// `Registry.setValue(name, value)` — persiste el par. Devuelve bool.
fn registry_set_value_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if name.is_empty() {
        return Ok(JsValue::from(false));
    }
    let value = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let mut map = registry_load(ctx);
    map.insert(name, serde_json::Value::String(value));
    Ok(JsValue::from(registry_store(ctx, &map)))
}

/// `Registry.exists(name)` — true si la clave existe.
fn registry_exists_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let map = registry_load(ctx);
    Ok(JsValue::from(map.contains_key(&name)))
}

/// `Registry.getKeys()` — array con los nombres de clave.
fn registry_get_keys_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let map = registry_load(ctx);
    let keys: Vec<String> = map.keys().cloned().collect();
    let json = serde_json::to_string(&keys).unwrap_or_else(|_| "[]".to_string());
    let arr = ctx
        .eval(boa_engine::Source::from_bytes(json.as_bytes()))
        .unwrap_or(JsValue::undefined());
    Ok(arr)
}

/// `Registry.deleteValue(name)` — borra la clave. Devuelve bool (existía).
fn registry_delete_value_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let mut map = registry_load(ctx);
    let existed = map.remove(&name).is_some();
    if existed {
        registry_store(ctx, &map);
    }
    Ok(JsValue::from(existed))
}

/// `Registry.clear()` — vacía todo el registro del script.
fn registry_clear_fn(_this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let empty = serde_json::Map::new();
    Ok(JsValue::from(registry_store(ctx, &empty)))
}

/// `Room_broadcast(text)` — alias de `sendPublic("Bot", text)`.
/// (Equivalente al sb0t original; ahora el bot name se puede custom.)
fn room_broadcast_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let text = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    if let Some(app) = lookup_app(ctx) {
        let from = app.settings.bot_name.clone();
        broadcast_to_users(&app, |c| server_core::outbound::build_public_c(&from, &text, c));
        Ok(JsValue::from(true))
    } else {
        Ok(JsValue::from(false))
    }
}

// ============================================================================
// Avatar / object class helpers (Fase 18)
// ============================================================================

/// Almacén thread-local de avatares creados vía `Avatar_new(bytes)`.
/// Los avatares son bytes en memoria asociados a un id. Se pueden
/// asociar a un user con `Avatar_setForUser`.
thread_local! {
    static AVATAR_STORE: std::cell::RefCell<Vec<Vec<u8>>>
        = std::cell::RefCell::new(Vec::new());
}

/// `Avatar_new(bytes_b64)` — crea un avatar en memoria a partir de bytes
/// en base64. Retorna el `id` (índice) del avatar, o `-1` si el input
/// no es base64 válido.
fn avatar_new_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match base64_decode_bytes(&s) {
        Some(bytes) => {
            let id = AVATAR_STORE.with(|store| {
                let mut s = store.borrow_mut();
                s.push(bytes);
                (s.len() - 1) as i32
            });
            Ok(JsValue::from(id))
        }
        None => Ok(JsValue::from(-1)),
    }
}

/// `Avatar_getSize(id)` — devuelve el tamaño en bytes del avatar `id`.
fn avatar_get_size_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    if id < 0 {
        return Ok(JsValue::from(-1));
    }
    let size = AVATAR_STORE.with(|store| {
        store.borrow().get(id as usize).map(|b| b.len() as i64)
    });
    Ok(JsValue::from(size.unwrap_or(-1)))
}

/// `Avatar_setForUser(name, avatar_id)` — asocia un avatar a un user.
/// El avatar se guarda en `AresUser.avatar`. Retorna `true` si OK.
fn avatar_set_for_user_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let id = args.get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    if id < 0 || name.is_empty() {
        return Ok(JsValue::from(false));
    }
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(false));
    };
    let bytes = AVATAR_STORE.with(|store| {
        store.borrow().get(id as usize).cloned()
    });
    let Some(bytes) = bytes else {
        return Ok(JsValue::from(false));
    };
    if let Some(user) = app.user_pool.get_by_name(&name) {
        *user.avatar.lock() = Some(bytes);
        Ok(JsValue::from(true))
    } else {
        Ok(JsValue::from(false))
    }
}

/// `Avatar_getForUser(name)` — devuelve el avatar (base64) de un user,
/// o `null` si no tiene avatar.
fn avatar_get_for_user_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let name = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::null());
    };
    if let Some(user) = app.user_pool.get_by_name(&name) {
        if let Some(bytes) = user.avatar.lock().clone() {
            return Ok(JsValue::from(js_string!(base64_encode_bytes_to_string(&bytes))));
        }
    }
    Ok(JsValue::null())
}

/// `Avatar_save(id, path)` — guarda un avatar del store a un archivo en disco.
/// Retorna `true` si OK.
fn avatar_save_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    let path = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if id < 0 || path.is_empty() {
        return Ok(JsValue::from(false));
    }
    let bytes = AVATAR_STORE.with(|store| {
        store.borrow().get(id as usize).cloned()
    });
    match bytes {
        Some(b) => match std::fs::write(&path, &b) {
            Ok(_) => Ok(JsValue::from(true)),
            Err(e) => {
                tracing::warn!("Avatar_save: error escribiendo {}: {}", path, e);
                Ok(JsValue::from(false))
            }
        },
        None => Ok(JsValue::from(false)),
    }
}

/// `Avatar_getBytes(id)` — devuelve los bytes del avatar como string base64.
fn avatar_get_bytes_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    if id < 0 {
        return Ok(JsValue::null());
    }
    let bytes = AVATAR_STORE.with(|store| {
        store.borrow().get(id as usize).cloned()
    });
    Ok(JsValue::from(js_string!(
        bytes.map(|b| base64_encode_bytes_to_string(&b))
            .unwrap_or_default()
    )))
}

// ============================================================================
// ScribbleImage (Fase 18) — clase de objeto real para scribbles
// ============================================================================

/// Almacén thread-local de scribbles (id → PNG bytes).
thread_local! {
    static SCRIBBLE_STORE: std::cell::RefCell<Vec<Vec<u8>>>
        = std::cell::RefCell::new(Vec::new());
}

/// `ScribbleImage_new(bytes_b64)` — crea una imagen de scribble desde
/// base64. Retorna el `id` o `-1` si el input es inválido.
fn scribble_image_new_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let s = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    match base64_decode_bytes(&s) {
        Some(bytes) => {
            let id = SCRIBBLE_STORE.with(|store| {
                let mut s = store.borrow_mut();
                s.push(bytes);
                (s.len() - 1) as i32
            });
            Ok(JsValue::from(id))
        }
        None => Ok(JsValue::from(-1))
    }
}

/// `ScribbleImage_getSize(id)` — tamaño en bytes del scribble.
fn scribble_image_get_size_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    if id < 0 {
        return Ok(JsValue::from(-1));
    }
    let size = SCRIBBLE_STORE.with(|store| {
        store.borrow().get(id as usize).map(|b| b.len() as i64)
    });
    Ok(JsValue::from(size.unwrap_or(-1)))
}

/// `ScribbleImage_save(id, path)` — guarda scribble a disco.
fn scribble_image_save_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    let path = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if id < 0 || path.is_empty() {
        return Ok(JsValue::from(false));
    }
    let bytes = SCRIBBLE_STORE.with(|store| {
        store.borrow().get(id as usize).cloned()
    });
    match bytes {
        Some(b) => match std::fs::write(&path, &b) {
            Ok(_) => Ok(JsValue::from(true)),
            Err(e) => {
                tracing::warn!("ScribbleImage_save error: {}", e);
                Ok(JsValue::from(false))
            }
        },
        None => Ok(JsValue::from(false)),
    }
}

// ============================================================================
// Query / Sql (Fase 19) — read-only DB access para scripts
// ============================================================================

/// Almacén thread-local de queries recientes: id → JSON array de resultados.
thread_local! {
    static QUERY_STORE: std::cell::RefCell<std::collections::HashMap<i32, String>>
        = std::cell::RefCell::new(std::collections::HashMap::new());
    static QUERY_COUNTER: std::cell::RefCell<i32> = std::cell::RefCell::new(0);
}

/// `Query_new(sql)` — ejecuta un SELECT (solo lectura) sobre la DB.
/// Retorna el `id` de la query (>= 0) en éxito, o `-1` si la query
/// no es SELECT o falla. Los resultados se acceden con `Query_getResults(id)`.
fn query_new_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let sql = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let trimmed = sql.trim_start().to_uppercase();
    // Solo permitir SELECT / WITH / EXPLAIN
    if !trimmed.starts_with("SELECT")
        && !trimmed.starts_with("WITH")
        && !trimmed.starts_with("EXPLAIN")
    {
        tracing::warn!("Query_new: solo se permiten queries SELECT/WITH/EXPLAIN");
        return Ok(JsValue::from(-1));
    }
    let Some(app) = lookup_app(ctx) else {
        return Ok(JsValue::from(-1));
    };
    match app.db.execute_select(&sql) {
        Ok((col_names, rows)) => {
            let mut json_rows = Vec::new();
            for row in &rows {
                let mut json_row = Vec::new();
                for (i, v) in row.iter().enumerate() {
                    let col = &col_names[i];
                    json_row.push(format!("\"{}\":{}", col, sqlite_value_to_json(v)));
                }
                json_rows.push(format!("{{{}}}", json_row.join(",")));
            }
            let json = format!("[{}]", json_rows.join(","));
            let id = QUERY_COUNTER.with(|c| {
                let mut c = c.borrow_mut();
                *c += 1;
                let id = *c;
                QUERY_STORE.with(|s| {
                    s.borrow_mut().insert(id, json);
                });
                id
            });
            Ok(JsValue::from(id))
        }
        Err(e) => {
            tracing::warn!("Query_new: {}", e);
            Ok(JsValue::from(-1))
        }
    }
}

// ============================================================================
// Sql — DB SQLite propia del script (paridad sb0t: new Sql() + new Query())
// ============================================================================
//
// El objeto JS `Sql` (definido en el prelude de compatibilidad) delega en estas
// funciones nativas, keyeadas por un handle numérico. Cada `Sql` abre un archivo
// SQLite en `<carpeta_del_script>/sql/<archivo>`. Los resultados de un SELECT se
// materializan y se recorren con `canRead`/`value` (como el Reader de sb0t).

struct SqlState {
    conn: Option<rusqlite::Connection>,
    cols: Vec<String>,
    rows: Vec<Vec<rusqlite::types::Value>>,
    cursor: i64,
    last_error: String,
}

thread_local! {
    static SQL_STORE: RefCell<std::collections::HashMap<u64, SqlState>> =
        RefCell::new(std::collections::HashMap::new());
    static SQL_COUNTER: RefCell<u64> = const { RefCell::new(0) };
}

fn sql_handle(args: &[JsValue]) -> u64 {
    args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0) as u64
}

fn sql_set_error(h: u64, msg: &str) {
    SQL_STORE.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&h) {
            st.last_error = msg.to_string();
        }
    });
}

fn sql_new_fn(_this: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = SQL_COUNTER.with(|c| {
        let mut c = c.borrow_mut();
        *c += 1;
        *c
    });
    SQL_STORE.with(|s| {
        s.borrow_mut().insert(
            id,
            SqlState { conn: None, cols: vec![], rows: vec![], cursor: -1, last_error: String::new() },
        );
    });
    Ok(JsValue::from(id as f64))
}

fn sql_open_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let h = sql_handle(args);
    let file = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    // No permitir separadores ni traversal: la DB vive en <script>/sql/.
    if file.is_empty() || file.contains('/') || file.contains('\\') || file.contains("..") {
        sql_set_error(h, "invalid file name");
        return Ok(JsValue::from(false));
    }
    let Some(dir) = current_script_dir(ctx) else {
        sql_set_error(h, "no script dir");
        return Ok(JsValue::from(false));
    };
    let sqldir = dir.join("sql");
    if let Err(e) = std::fs::create_dir_all(&sqldir) {
        sql_set_error(h, &format!("mkdir sql: {}", e));
        return Ok(JsValue::from(false));
    }
    let path = sqldir.join(&file);
    match rusqlite::Connection::open(&path) {
        Ok(conn) => {
            SQL_STORE.with(|s| {
                if let Some(st) = s.borrow_mut().get_mut(&h) {
                    st.conn = Some(conn);
                    st.last_error.clear();
                }
            });
            Ok(JsValue::from(true))
        }
        Err(e) => {
            sql_set_error(h, &e.to_string());
            Ok(JsValue::from(false))
        }
    }
}

/// Ejecuta una query (SELECT o DDL/DML) con parámetros posicionales.
fn sql_run(
    conn: &rusqlite::Connection,
    sql: &str,
    params: &[rusqlite::types::Value],
) -> Result<(Vec<String>, Vec<Vec<rusqlite::types::Value>>), String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let ncols = stmt.column_count();
    let cols: Vec<String> = (0..ncols)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();
    let mut out_rows = Vec::new();
    let mut rows = stmt
        .query(rusqlite::params_from_iter(params.iter()))
        .map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut r = Vec::with_capacity(ncols);
        for i in 0..ncols {
            r.push(row.get::<_, rusqlite::types::Value>(i).unwrap_or(rusqlite::types::Value::Null));
        }
        out_rows.push(r);
    }
    Ok((cols, out_rows))
}

fn sql_query_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let h = sql_handle(args);
    let sql_raw = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let params = sql_extract_params(args.get(2), ctx);
    // Sustituir los placeholders `{0}`,`{1}`... de sb0t por `?1`,`?2`...
    let mut sql = sql_raw;
    for i in 0..params.len() {
        sql = sql.replace(&format!("{{{}}}", i), &format!("?{}", i + 1));
    }
    SQL_STORE.with(|s| {
        let mut store = s.borrow_mut();
        let Some(st) = store.get_mut(&h) else {
            return Ok(JsValue::from(false));
        };
        let conn = match st.conn.take() {
            Some(c) => c,
            None => {
                st.last_error = "connection closed".to_string();
                return Ok(JsValue::from(false));
            }
        };
        let result = sql_run(&conn, &sql, &params);
        st.conn = Some(conn);
        match result {
            Ok((cols, rows)) => {
                st.cols = cols;
                st.rows = rows;
                st.cursor = -1;
                st.last_error.clear();
                Ok(JsValue::from(true))
            }
            Err(e) => {
                st.cols.clear();
                st.rows.clear();
                st.cursor = -1;
                st.last_error = e;
                Ok(JsValue::from(false))
            }
        }
    })
}

fn sql_can_read_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let h = sql_handle(args);
    let can = SQL_STORE.with(|s| {
        let mut store = s.borrow_mut();
        match store.get_mut(&h) {
            Some(st) => {
                st.cursor += 1;
                (st.cursor as usize) < st.rows.len()
            }
            None => false,
        }
    });
    Ok(JsValue::from(can))
}

fn sql_value_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    use rusqlite::types::Value;
    let h = sql_handle(args);
    let col = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    SQL_STORE.with(|s| {
        let store = s.borrow();
        let Some(st) = store.get(&h) else {
            return Ok(JsValue::null());
        };
        if st.cursor < 0 || (st.cursor as usize) >= st.rows.len() {
            return Ok(JsValue::null());
        }
        let Some(idx) = st.cols.iter().position(|c| c == &col) else {
            return Ok(JsValue::null());
        };
        let v = &st.rows[st.cursor as usize][idx];
        let js = match v {
            Value::Null => JsValue::null(),
            Value::Integer(i) => JsValue::from(js_string!(i.to_string())),
            Value::Real(f) => JsValue::from(js_string!(f.to_string())),
            Value::Text(t) => JsValue::from(js_string!(t.clone())),
            Value::Blob(b) => JsValue::from(js_string!(base64_encode_bytes_to_string(b))),
        };
        Ok(js)
    })
}

fn sql_close_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let h = sql_handle(args);
    SQL_STORE.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&h) {
            st.conn = None; // cierra la conexión
            st.rows.clear();
            st.cols.clear();
            st.cursor = -1;
        }
    });
    Ok(JsValue::from(true))
}

fn sql_last_error_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let h = sql_handle(args);
    let e = SQL_STORE.with(|s| s.borrow().get(&h).map(|st| st.last_error.clone()).unwrap_or_default());
    Ok(JsValue::from(js_string!(e)))
}

/// Convierte un array JS de parámetros a valores SQLite.
fn sql_extract_params(v: Option<&JsValue>, ctx: &mut Context) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value;
    let mut out = Vec::new();
    let Some(v) = v else { return out };
    let Some(obj) = v.as_object() else { return out };
    let len = obj
        .get(js_string!("length"), ctx)
        .ok()
        .and_then(|l| l.as_number())
        .unwrap_or(0.0) as u32;
    for i in 0..len {
        let elem = obj.get(i, ctx).unwrap_or(JsValue::null());
        if elem.is_null() || elem.is_undefined() {
            out.push(Value::Null);
        } else if let Some(n) = elem.as_number() {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                out.push(Value::Integer(n as i64));
            } else {
                out.push(Value::Real(n));
            }
        } else {
            out.push(Value::Text(jsvalue_to_string(&elem).unwrap_or_default()));
        }
    }
    out
}

/// Convierte un `rusqlite::types::Value` a string JSON.
fn sqlite_value_to_json(v: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match v {
        Value::Null => "null".to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Blob(b) => format!("\"<blob {} bytes>\"", b.len()),
    }
}

/// `Query_getResults(id)` — devuelve el JSON array con los resultados
/// de la query `id`, o `"null"` si no existe.
fn query_get_results_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    let json = QUERY_STORE.with(|s| s.borrow().get(&id).cloned());
    Ok(JsValue::from(js_string!(json.unwrap_or_else(|| "null".to_string()))))
}

/// `Query_getColumnCount(id)` — devuelve la cantidad de columnas del
/// resultado de la query `id`, o `-1` si no existe.
fn query_get_column_count_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    // Contar comas top-level + 1 (asume JSON simple sin comillas dentro)
    let count = QUERY_STORE.with(|s| {
        s.borrow().get(&id).map(|json| {
            // Para contar columnas, parsear la primera fila
            if let Some(start) = json.find('[') {
                if let Some(end) = json.find(']') {
                    let inner = &json[start+1..end];
                    if let Some(first_row_end) = inner.find('}') {
                        let first_row = &inner[..=first_row_end];
                        first_row.matches(':').count() as i32
                    } else { 0 }
                } else { 0 }
            } else { 0 }
        })
    });
    Ok(JsValue::from(count.unwrap_or(-1)))
}

/// `Query_getRowCount(id)` — devuelve la cantidad de filas del resultado.
fn query_get_row_count_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    let count = QUERY_STORE.with(|s| {
        s.borrow().get(&id).map(|json| {
            // Contar `}` que cierran objetos top-level (dentro del array).
            // Después de decrementar, depth==1 significa que estamos en el nivel del array.
            let mut depth: i32 = 0;
            let mut count: i32 = 0;
            let mut in_string = false;
            let mut escape = false;
            for c in json.chars() {
                if escape { escape = false; continue; }
                if c == '\\' && in_string { escape = true; continue; }
                if c == '"' { in_string = !in_string; continue; }
                if in_string { continue; }
                match c {
                    '{' | '[' => depth += 1,
                    '}' | ']' => {
                        depth -= 1;
                        if c == '}' && depth == 1 { count += 1; }
                    }
                    _ => {}
                }
            }
            count
        })
    });
    Ok(JsValue::from(count.unwrap_or(-1) as i64))
}

// ============================================================================
// Help extend (Fase 20) — agregar líneas a /help desde scripts
// ============================================================================

/// `Help_addLine(command, line)` — agrega una línea custom al comando `/help`.
/// Ej: `Help_addLine("hola", "/hola - saluda al bot")` → agrega esa línea
/// cuando el user escribe `/help`. Retorna true si se agregó.
fn help_add_line_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let cmd = args.get(0).and_then(jsvalue_to_string).unwrap_or_default();
    let line = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if cmd.is_empty() || line.is_empty() {
        return Ok(JsValue::from(false));
    }
    let script = current_script_name(ctx);
    if let Ok(mut lines) = HELP_LINES.lock() {
        let entry = (script, cmd, line);
        // Dedup: evitar duplicados si un script se recarga y re-registra.
        if !lines.contains(&entry) {
            lines.push(entry);
        }
    }
    Ok(JsValue::from(true))
}

/// Elimina las líneas de `/help` registradas por un script. Se llama al
/// descargarlo (o antes de recargarlo), para que no se acumulen entradas de
/// instancias muertas.
pub fn clear_help_lines_for(script: &str) {
    if let Ok(mut lines) = HELP_LINES.lock() {
        lines.retain(|(s, _, _)| s != script);
    }
}

/// Retorna una copia de las líneas extra de help registradas por scripts.
/// Usado por `handle_help` en el crate `commands` para agregar líneas
/// antes de mandar el PM al user. Global → visible desde cualquier hilo.
pub fn extra_help_lines() -> Vec<(String, String)> {
    HELP_LINES
        .lock()
        .map(|l| l.iter().map(|(_, c, t)| (c.clone(), t.clone())).collect())
        .unwrap_or_default()
}

// ============================================================================
// Timer (Fase 20) — one-shot timers que disparan onTimer
// ============================================================================

/// `setTimer(secs, fn_name)` — agenda la función JS `fn_name` para que se
/// ejecute cada `secs` segundos (repeating). Retorna un id (>0) que se
/// puede usar con `clearTimer(id)` para cancelar.
fn set_timer_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let secs = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u64)
        .unwrap_or(0);
    let fn_name = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if secs == 0 || fn_name.is_empty() {
        return Ok(JsValue::from(-1));
    }
    let id = TIMER_COUNTER.with(|c| {
        let mut c = c.borrow_mut();
        *c += 1;
        *c
    });
    let script = current_script_name(ctx);
    ACTIVE_TIMERS.with(|t| t.borrow_mut().insert(id));
    PENDING_TIMERS.with(|t| {
        t.borrow_mut().push_back(PendingTimer {
            id,
            fn_name,
            fire_at: std::time::Instant::now() + std::time::Duration::from_secs(secs),
            repeat: true,
            script,
        });
    });
    Ok(JsValue::from(id))
}

/// `setTimeout(secs, fn_name)` — agenda la función JS `fn_name` para que se
/// ejecute UNA SOLA VEZ después de `secs` segundos. Retorna un id.
fn set_timeout_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let secs = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as u64)
        .unwrap_or(0);
    let fn_name = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    if secs == 0 || fn_name.is_empty() {
        return Ok(JsValue::from(-1));
    }
    let id = TIMER_COUNTER.with(|c| {
        let mut c = c.borrow_mut();
        *c += 1;
        *c
    });
    let script = current_script_name(ctx);
    ACTIVE_TIMERS.with(|t| t.borrow_mut().insert(id));
    PENDING_TIMERS.with(|t| {
        t.borrow_mut().push_back(PendingTimer {
            id,
            fn_name,
            fire_at: std::time::Instant::now() + std::time::Duration::from_secs(secs),
            repeat: false,
            script,
        });
    });
    Ok(JsValue::from(id))
}

/// Timer pendiente de procesar (Fase 20).
#[derive(Debug, Clone)]
pub struct PendingTimer {
    /// ID único del timer
    pub id: i32,
    /// Nombre de la función JS a llamar
    pub fn_name: String,
    /// Momento en que debe dispararse
    pub fire_at: std::time::Instant,
    /// Si es repeating, se re-encola después de disparar.
    pub repeat: bool,
    /// Script que lo agendó (para cancelarlo si se descarga/recarga).
    pub script: String,
}

/// Cancela todos los timers agendados por un script. Se llama al
/// descargarlo: si no, los timers repetitivos de una instancia muerta
/// siguen re-encolándose para siempre.
pub fn clear_timers_for(script: &str) {
    PENDING_TIMERS.with(|t| {
        t.borrow_mut().retain(|timer| timer.script != script);
    });
}

thread_local! {
    /// Cola de timers pendientes. Procesada por el manager.
    static PENDING_TIMERS: std::cell::RefCell<std::collections::VecDeque<PendingTimer>>
        = std::cell::RefCell::new(std::collections::VecDeque::new());
}

/// Pop un timer pendiente. Llamado por el manager en su loop.
pub fn pop_pending_timer() -> Option<PendingTimer> {
    PENDING_TIMERS.with(|t| t.borrow_mut().pop_front())
}

/// Pop timers que ya expiraron. Mantiene los futuros en la cola.
pub fn pop_due_timers(now: std::time::Instant) -> Vec<PendingTimer> {
    let mut due = Vec::new();
    PENDING_TIMERS.with(|t| {
        let mut queue = t.borrow_mut();
        // Particionar en due y no-due
        let mut i = 0;
        while i < queue.len() {
            if queue[i].fire_at <= now {
                due.push(queue.remove(i).unwrap());
            } else {
                i += 1;
            }
        }
    });
    due
}

/// Push un timer de vuelta a la cola (para repeating timers).
pub fn push_pending_timer(timer: PendingTimer) {
    PENDING_TIMERS.with(|t| t.borrow_mut().push_back(timer));
}

/// `clearTimer(id)` — cancela un timer agendado. Retorna `true` si el id
/// existía, `false` si no (ya expiró o nunca existió).
fn clear_timer_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let id = args.get(0)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(-1);
    let removed = ACTIVE_TIMERS.with(|t| t.borrow_mut().remove(&id));
    Ok(JsValue::from(removed))
}

// ============================================================================
// HTTP async (HttpRequest / ProxyCheck, compat sb0t) — Fase 3b
// ============================================================================
//
// El engine de scripting corre en un thread dedicado (no async). Para no
// bloquearlo, `__http_download` lanza un std::thread que hace la petición
// con reqwest::blocking y deja el resultado en una cola GLOBAL. El manager
// drena esa cola en cada dispatch (igual que los timers) y emite un
// `ScriptEvent::HttpComplete` → el prelude enruta al callback por `key`.

/// Resultado de una petición HTTP completada en background.
pub struct HttpCompletion {
    /// Clave del callback registrado en el context que originó la petición.
    pub key: String,
    /// Cuerpo (texto UTF-8 o base64 de los bytes crudos según `utf`).
    pub body: String,
    /// Código de estado HTTP (0 si error de red).
    pub status: u16,
    /// Mensaje de error, vacío si OK.
    pub error: String,
}

static PENDING_HTTP: std::sync::LazyLock<std::sync::Mutex<std::collections::VecDeque<HttpCompletion>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::VecDeque::new()));

/// Drena las respuestas HTTP completadas (llamado por el manager en su thread).
pub fn drain_http_completions() -> Vec<HttpCompletion> {
    let mut q = PENDING_HTTP.lock().unwrap();
    q.drain(..).collect()
}

/// `__http_download(method, url, body, userAgent, accept, utf, key)` — lanza
/// la petición en background. `utf` = true → cuerpo como texto; false → base64.
/// Devuelve true si se pudo lanzar.
fn http_download_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> Result<JsValue, boa_engine::JsError> {
    let method = args.get(0).and_then(jsvalue_to_string).unwrap_or_else(|| "GET".to_string());
    let url = args.get(1).and_then(jsvalue_to_string).unwrap_or_default();
    let body = args.get(2).and_then(jsvalue_to_string).unwrap_or_default();
    let user_agent = args.get(3).and_then(jsvalue_to_string).unwrap_or_default();
    let accept = args.get(4).and_then(jsvalue_to_string).unwrap_or_default();
    let utf = args.get(5).map(|v| v.to_boolean()).unwrap_or(true);
    let key = args.get(6).and_then(jsvalue_to_string).unwrap_or_default();
    // Headers custom (JSON {nombre: valor}, paridad sb0t `req.header(n, v)`).
    let headers_json = args.get(7).and_then(jsvalue_to_string).unwrap_or_default();
    if url.is_empty() || key.is_empty() {
        return Ok(JsValue::from(false));
    }

    let spawned = std::thread::Builder::new()
        .name("script-http".into())
        .spawn(move || {
            let mut completion = HttpCompletion {
                key: key.clone(),
                body: String::new(),
                status: 0,
                error: String::new(),
            };
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build();
            match client {
                Ok(client) => {
                    let m = method.to_uppercase();
                    let mut req = if m == "POST" {
                        client.post(&url).body(body)
                    } else {
                        client.get(&url)
                    };
                    if !user_agent.is_empty() {
                        req = req.header(reqwest::header::USER_AGENT, user_agent);
                    }
                    if !accept.is_empty() {
                        req = req.header(reqwest::header::ACCEPT, accept);
                    }
                    if !headers_json.is_empty() {
                        if let Ok(serde_json::Value::Object(map)) =
                            serde_json::from_str::<serde_json::Value>(&headers_json)
                        {
                            for (k, v) in map {
                                if let Some(val) = v.as_str() {
                                    req = req.header(k, val);
                                }
                            }
                        }
                    }
                    match req.send() {
                        Ok(resp) => {
                            completion.status = resp.status().as_u16();
                            match resp.bytes() {
                                Ok(bytes) => {
                                    completion.body = if utf {
                                        String::from_utf8_lossy(&bytes).into_owned()
                                    } else {
                                        base64_encode_bytes_to_string(&bytes)
                                    };
                                }
                                Err(e) => completion.error = format!("read body: {}", e),
                            }
                        }
                        Err(e) => completion.error = e.to_string(),
                    }
                }
                Err(e) => completion.error = format!("client build: {}", e),
            }
            PENDING_HTTP.lock().unwrap().push_back(completion);
        });
    Ok(JsValue::from(spawned.is_ok()))
}

// ============================================================================
// Helpers
// ============================================================================

fn jsvalue_to_string(v: &JsValue) -> Option<String> {
    v.as_string().map(|s| s.to_std_string_escaped())
}

fn format_js_value(v: &JsValue) -> String {
    if v.is_undefined() {
        "undefined".into()
    } else if v.is_null() {
        "null".into()
    } else if let Some(s) = v.as_string() {
        s.to_std_string_escaped()
    } else if v.is_boolean() {
        v.as_boolean().unwrap().to_string()
    } else if v.is_number() {
        v.as_number().unwrap().to_string()
    } else {
        format!("{:?}", v)
    }
}

/// Broadcast de un paquete a todos los users logueados.
/// Difunde un paquete a todos los usuarios logueados, construyéndolo
/// por-destinatario para que cada uno reciba sus strings cifrados con su
/// propia key si negoció AES (`user.ares_crypto`).
fn broadcast_to_users<F>(app: &AppContext, build: F)
where
    F: Fn(server_core::outbound::Crypto) -> Bytes,
{
    for u in app.user_pool.users() {
        if u.logged_in {
            let _ = u.send(build(u.ares_crypto));
        }
    }
}

// Base64 helpers (independientes, no usan la registry)
fn base64_encode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut buf = Vec::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        buf.push(B64[(b0 >> 2) as usize]);
        buf.push(B64[((b0 << 4 | b1 >> 4) & 0x3F) as usize]);
        if chunk.len() > 1 {
            buf.push(B64[((b1 << 2 | b2 >> 6) & 0x3F) as usize]);
        } else {
            buf.push(b'=');
        }
        if chunk.len() > 2 {
            buf.push(B64[(b2 & 0x3F) as usize]);
        } else {
            buf.push(b'=');
        }
    }
    String::from_utf8(buf).unwrap_or_default()
}

fn base64_decode(s: &str) -> Option<String> {
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            buf[i] = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => 0,
                _ => return None,
            };
        }
        let n = chunk.iter().filter(|&&c| c != b'=').count();
        let combined = ((buf[0] as u32) << 18)
            | ((buf[1] as u32) << 12)
            | ((buf[2] as u32) << 6)
            | (buf[3] as u32);
        if n >= 2 { out.push((combined >> 16) as u8); }
        if n >= 3 { out.push((combined >> 8) as u8); }
        if n >= 4 { out.push(combined as u8); }
    }
    String::from_utf8(out).ok()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// ============================================================================
// eval_script + call_global_function
// ============================================================================

/// Evalúa código fuente JS en el contexto.
pub fn eval_script(ctx: &mut Context, source: &str) -> Result<(), String> {
    ctx.eval(boa_engine::Source::from_bytes(source.as_bytes()))
        .map_err(|e| format!("eval error: {}", e))?;
    Ok(())
}

/// Como `eval_script` pero devuelve el valor resultante como string (debug).
#[cfg(test)]
pub fn eval_script_value(ctx: &mut Context, source: &str) -> Result<String, String> {
    let v = ctx
        .eval(boa_engine::Source::from_bytes(source.as_bytes()))
        .map_err(|e| format!("eval error: {}", e))?;
    Ok(v.to_string(ctx).map(|s| s.to_std_string_lossy()).unwrap_or_default())
}

/// Construye el objeto `user` (JSUser) para `name` invocando el global
/// `user(name)` del prelude. Si el prelude no está disponible, cae al string
/// del nombre. Se usa para pasar un JSUser a los handlers (paridad sb0t);
/// el objeto tiene toString/valueOf = nombre, así que sigue funcionando en
/// contexto string.
pub fn build_user_object(ctx: &mut Context, name: &str) -> JsValue {
    // Nombre vacío = "sin usuario" (p.ej. target no resuelto de onCommand):
    // el handler recibe null, como en sb0t.
    if name.is_empty() {
        return JsValue::null();
    }
    let user_fn = match ctx.global_object().get(js_string!("user"), ctx) {
        Ok(v) if v.is_object() => v,
        _ => return JsValue::from(js_string!(name)),
    };
    let obj = match user_fn.as_object() {
        Some(o) => o.clone(),
        None => return JsValue::from(js_string!(name)),
    };
    let arg = JsValue::from(js_string!(name));
    match obj.call(&JsValue::undefined(), &[arg], ctx) {
        Ok(u) if !u.is_null_or_undefined() => u,
        _ => JsValue::from(js_string!(name)),
    }
}

/// Construye el objeto `PM` (JSPM) para `text` invocando el global `__mkPM`
/// del prelude (un String con helpers contains/remove/replace/isScribble).
/// Cae al string plano si el prelude no está disponible.
pub fn build_pm_object(ctx: &mut Context, text: &str) -> JsValue {
    let mk = match ctx.global_object().get(js_string!("__mkPM"), ctx) {
        Ok(v) if v.is_object() => v,
        _ => return JsValue::from(js_string!(text)),
    };
    let obj = match mk.as_object() {
        Some(o) => o.clone(),
        None => return JsValue::from(js_string!(text)),
    };
    let arg = JsValue::from(js_string!(text));
    match obj.call(&JsValue::undefined(), &[arg], ctx) {
        Ok(v) if !v.is_null_or_undefined() => v,
        _ => JsValue::from(js_string!(text)),
    }
}

/// Llama a una función JS global con los argumentos dados.
pub fn call_global_function(
    ctx: &mut Context,
    name: &str,
    args: &[JsValue],
) -> Result<(), String> {
    let key = boa_engine::property::PropertyKey::from(js_string!(name));
    let func = ctx
        .global_object()
        .get(key, ctx)
        .map_err(|e| format!("error buscando '{}': {}", name, e))?;

    if func.is_undefined() || func.is_null() {
        return Ok(());
    }
    if !func.is_object() {
        return Err(format!("'{}' no es una función", name));
    }
    let _keep_alive = func.as_object().unwrap().clone();

    let key2 = boa_engine::property::PropertyKey::from(js_string!(name));
    let result = ctx
        .global_object()
        .get(key2, ctx)
        .map_err(|e| format!("error buscando '{}': {}", name, e))?
        .as_object()
        .unwrap()
        .call(&JsValue::undefined(), args, ctx)
        .map_err(|e| format!("error ejecutando '{}': {}", name, e))?;
    let _ = result;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;

    fn make_app() -> Arc<AppContext> {
        let db = Database::in_memory().unwrap();
        Arc::new(AppContext::new(Settings::default(), db))
    }

    #[test]
    fn create_context_and_print() {
        let mut ctx = make_context(make_app());
        let result = eval_script(&mut ctx, r#"print("hello from JS");"#);
        assert!(result.is_ok(), "eval should succeed: {:?}", result);
    }

    #[test]
    fn user_count_real() {
        let app = make_app();
        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            const n = userCount();
            if (typeof n !== 'number') throw 'not a number';
            if (n !== 0) throw 'expected 0, got ' + n;
        "#,
        );
        assert!(result.is_ok(), "eval should succeed: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn define_onpublic_handler() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            function onPublic(from, text) {
                log("got public from " + from + ": " + text);
            }
        "#,
        );
        assert!(result.is_ok(), "should be able to define handler: {:?}", result);
    }

    #[test]
    fn call_global_function_works() {
        let mut ctx = make_context(make_app());
        eval_script(
            &mut ctx,
            r#"
            var captured = null;
            function onPublic(from, text) {
                captured = from + ':' + text;
            }
        "#,
        )
        .unwrap();

        let from = JsValue::from(js_string!("Alice"));
        let text = JsValue::from(js_string!("hello"));
        let result = call_global_function(&mut ctx, "onPublic", &[from, text]);
        assert!(result.is_ok(), "call should succeed: {:?}", result);
    }

    #[test]
    fn call_missing_handler_is_ok() {
        let mut ctx = make_context(make_app());
        let result = call_global_function(&mut ctx, "onNonExistent", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn syntax_error_caught() {
        let mut ctx = make_context(make_app());
        let result = eval_script(&mut ctx, "function { invalid syntax");
        assert!(result.is_err(), "should fail on syntax error");
    }

    #[test]
    fn registry_lookup_works() {
        let app = make_app();
        let ctx = Context::default();
        assert!(lookup_app(&ctx).is_none());
        register_context(&ctx, &app);
        let found = lookup_app(&ctx);
        assert!(found.is_some());
        assert_eq!(found.unwrap().settings.port, app.settings.port);
        unregister_context(&ctx);
        assert!(lookup_app(&ctx).is_none());
    }

    // ========== Tests de las nuevas APIs ==========

    #[test]
    fn sb0t_compat_phase2_apis() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            // Objeto user: existe, tiene props (null sin user en pool) y métodos.
            var u = user("Nobody");
            if (u == null) throw "user() returned null";
            if (u.name !== null) throw "expected null name, got " + u.name;
            if (typeof u.ban !== "function") throw "ban is not a function";
            if (typeof u.kick !== "function") throw "kick is not a function";
            if (typeof u.sendText !== "function") throw "sendText is not a function";
            if (Users.getUserByName("x") == null) throw "Users.getUserByName null";

            // Room getters
            if (typeof Room.name() !== "string") throw "Room.name not string";
            if (typeof Room.port() !== "number") throw "Room.port not number";
            if (typeof Room.version() !== "string") throw "Room.version not string";
            if (typeof Room.startTime() !== "number") throw "Room.startTime not number";

            // Stats
            if (Stats.userCount() !== 0) throw "Stats.userCount != 0";
            if (typeof Stats.peakUserCount() !== "number") throw "Stats.peak not number";

            // List
            var L = new List();
            L.add(1).add(2).addRange([3, 4]);
            if (L.count !== 4) throw "List.count != 4: " + L.count;
            if (L.length !== 4) throw "List.length != 4";
            if (L.indexOf(3) !== 2) throw "List.indexOf(3) != 2";
            if (L.find(function(x){ return x > 2; }) !== 3) throw "List.find bad";
            if (L.findIndex(function(x){ return x === 4; }) !== 3) throw "List.findIndex bad";
            L.removeAt(0);
            if (L.count !== 3 || L.get(0) !== 2) throw "List.removeAt bad";
            if (L.join("-") !== "2-3-4") throw "List.join bad: " + L.join("-");
            L.clear();
            if (L.count !== 0) throw "List.clear bad";

            // Globals
            if (escapeUtf("a b") !== "a%20b") throw "escapeUtf bad: " + escapeUtf("a b");
            if (typeof clrName("test") !== "string") throw "clrName not string";

            // Timer: construible, start/stop no lanzan
            var t = new Timer();
            t.interval = 5000;
            t.oncomplete = function(){};
            t.start();
            t.stop();
        "#,
        );
        assert!(result.is_ok(), "phase2 apis should work: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn sb0t_compat_phase3_apis() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            // Registry: KV persistente (en memoria sin script dir)
            if (Registry.exists("k") !== false) throw "exists should be false";
            if (Registry.setValue("k", "v1") !== true) throw "setValue failed";
            if (Registry.getValue("k") !== "v1") throw "getValue != v1: " + Registry.getValue("k");
            if (Registry.exists("k") !== true) throw "exists should be true";
            Registry.setValue("k2", "v2");
            var keys = Registry.getKeys();
            if (keys.indexOf("k") < 0 || keys.indexOf("k2") < 0) throw "getKeys missing";
            if (Registry.deleteValue("k") !== true) throw "deleteValue failed";
            if (Registry.exists("k") !== false) throw "k should be gone";
            Registry.clear();
            if (Registry.getKeys().length !== 0) throw "clear failed";
            if (Registry.getValue("nope") !== null) throw "missing key should be null";

            // XmlParser: parseo + navegación
            var p = new XmlParser();
            var ok = p.load('<root a="1"><item id="x">hello</item><item id="y">world</item></root>');
            if (ok !== true || p.available !== true) throw "xml load failed";
            if (p.nodeName !== "root") throw "root name: " + p.nodeName;
            if (p.attributes.a !== "1") throw "root attr a: " + p.attributes.a;
            var items = p.getNodesByName("item");
            if (items.length !== 2) throw "expected 2 items, got " + items.length;
            if (items[0].attributes.id !== "x") throw "item0 id: " + items[0].attributes.id;
            if (items[0].nodeValue !== "hello") throw "item0 value: " + items[0].nodeValue;
            if (items[1].nodeValue !== "world") throw "item1 value: " + items[1].nodeValue;

            // XmlParser: construcción
            var p2 = new XmlParser();
            var r = p2.create("config");
            var c = new XmlNode("entry"); c.nodeValue = "z";
            r.appendChild(c);
            if (p2.getNodesByName("entry").length !== 1) throw "appendChild failed";
            r.removeChild(c);
            if (p2.getNodesByName("entry").length !== 0) throw "removeChild failed";

            // Avatar / Scribble construibles desde base64 (JVBER = 4 bytes)
            var av = new Avatar(Base64_encode("abcd"));
            if (av.size !== 4) throw "avatar size: " + av.size;
            var sc = new Scribble(Base64_encode("abcdef"));
            if (sc.size !== 6) throw "scribble size: " + sc.size;
        "#,
        );
        assert!(result.is_ok(), "phase3 apis should work: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn sb0t_compat_phase5_apis() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            // Entities encode/decode
            if (Entities.encode('<a href="x">&') !== "&lt;a href=&quot;x&quot;&gt;&amp;")
                throw "entities.encode: " + Entities.encode('<a href="x">&');
            if (Entities.decode("&lt;b&gt;&amp;&quot;&#39;") !== "<b>&\"'")
                throw "entities.decode: " + Entities.decode("&lt;b&gt;&amp;&quot;&#39;");

            // Spelling.confirm agrega al diccionario runtime
            if (Spelling.check("zzqqxx") !== false) throw "unknown word should be false";
            Spelling.confirm("zzqqxx");
            if (Spelling.check("zzqqxx") !== true) throw "confirmed word should be true";

            // PM (JSPM): helpers + string-compat
            var pm = new PM("hello world");
            if (pm.contains("world") !== true) throw "pm.contains";
            if (("" + pm) !== "hello world") throw "pm toString";
            pm.replace("world", "there");
            if (("" + pm) !== "hello there") throw "pm.replace: " + pm;
            pm.remove("hello ");
            if (("" + pm) !== "there") throw "pm.remove: " + pm;
            if (new PM('#scribble#x').isScribble !== true) throw "pm.isScribble";

            // Users colecciones sin datos → arrays vacíos
            if (Users.records().length !== 0) throw "records not empty";
            if (Users.banned().length !== 0) throw "banned not empty";
            if (Users.linked().length !== 0) throw "linked not empty";

            // Channels.list/available devuelven array parseado
            if (!Array.isArray(Channels.list())) throw "Channels.list not array";
            if (Channels.enabled() !== true) throw "Channels.enabled";

            // Link stubs (Astra no linkea)
            if (Link.linked() !== false) throw "Link.linked should be false";
            if (Link.leaves().length !== 0) throw "Link.leaves not empty";

            // Leaf stub
            var lf = new Leaf();
            if (lf.users().length !== 0) throw "Leaf.users";
            if (lf.sendText("hi") !== false) throw "Leaf.sendText";
        "#,
        );
        assert!(result.is_ok(), "phase5 apis should work: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn http_request_end_to_end() {
        use std::io::{Read, Write};
        // Servidor HTTP local que responde una vez con un cuerpo fijo.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let body = "HELLO_HTTP";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let mut ctx = make_context(make_app());
        let url = format!("http://{}/", addr);
        eval_script(
            &mut ctx,
            &format!(
                r#"
                var __result = null; var __status = 0;
                var r = new HttpRequest();
                r.src = "{}";
                r.oncomplete = function(body, status, error){{ __result = body; __status = status; }};
                if (r.download() !== true) throw "download() should return true";
            "#,
                url
            ),
        )
        .unwrap();

        // Esperar a que el thread de background complete la petición.
        let mut completions = Vec::new();
        for _ in 0..100 {
            completions = drain_http_completions();
            if !completions.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(completions.len(), 1, "expected one HTTP completion");
        let c = &completions[0];
        assert_eq!(c.status, 200, "status should be 200, error={}", c.error);

        // Simular lo que hace el manager: enrutar la respuesta al callback.
        let args = [
            JsValue::from(js_string!(c.key.clone())),
            JsValue::from(js_string!(c.body.clone())),
            JsValue::from(js_string!(c.status.to_string())),
            JsValue::from(js_string!(c.error.clone())),
        ];
        call_global_function(&mut ctx, "onHttpComplete", &args).unwrap();

        let check = eval_script(
            &mut ctx,
            r#"
            if (__result !== "HELLO_HTTP") throw "body mismatch: " + __result;
            if (__status !== 200) throw "status mismatch: " + __status;
        "#,
        );
        assert!(check.is_ok(), "oncomplete should receive body: {:?}", check);
        srv.join().unwrap();
        unregister_context(&ctx);
    }

    #[test]
    fn sha1_fn_works() {
        let mut ctx = make_context(make_app());
        // SHA-1("hello") = aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d
        let result = eval_script(
            &mut ctx,
            r#"
            const h = astraHash("hello");
            if (h !== "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d") {
                throw "bad hash: " + h;
            }
        "#,
        );
        assert!(result.is_ok(), "sha1 should produce correct hash: {:?}", result);
    }

    #[test]
    fn md5_fn_works() {
        let mut ctx = make_context(make_app());
        // MD5("hello") = 5d41402abc4b2a76b9719d911017c592
        let result = eval_script(
            &mut ctx,
            r#"
            const h = astraMd5("hello");
            if (h !== "5d41402abc4b2a76b9719d911017c592") {
                throw "bad md5: " + h;
            }
        "#,
        );
        assert!(result.is_ok(), "md5 should produce correct hash: {:?}", result);
    }

    #[test]
    fn base64_roundtrip() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const enc = astraBase64Encode("hello world");
            const dec = astraBase64Decode(enc);
            if (dec !== "hello world") {
                throw "roundtrip failed: " + dec;
            }
            // Sanity check
            if (enc !== "aGVsbG8gd29ybGQ=") {
                throw "bad encoding: " + enc;
            }
        "#,
        );
        assert!(result.is_ok(), "base64 roundtrip should work: {:?}", result);
    }

    #[test]
    fn base64_decode_invalid() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const dec = astraBase64Decode("not valid base64 @@@");
            if (dec !== null) throw "expected null for invalid input";
        "#,
        );
        assert!(result.is_ok(), "invalid base64 should return null: {:?}", result);
    }

    #[test]
    fn get_topic_and_set_topic() {
        let app = make_app();
        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            const t0 = getTopic();
            if (typeof t0 !== 'string') throw 'getTopic should return string';
            const ok = setTopic("test topic");
            if (ok !== true) throw 'setTopic returned ' + ok;
            const t1 = getTopic();
            if (t1 !== "test topic") throw 'setTopic did not work, got: ' + t1;
        "#,
        );
        assert!(result.is_ok(), "topic get/set should work: {:?}", result);
    }

    #[test]
    fn user_exists_and_count() {
        let app = make_app();
        let mut ctx = make_context(app.clone());
        // Sin users
        let result = eval_script(
            &mut ctx,
            r#"
            if (userCount() !== 0) throw 'expected 0 users, got ' + userCount();
            if (userExists("nobody") !== false) throw 'nobody should not exist';
        "#,
        );
        assert!(result.is_ok(), "userExists/count should work: {:?}", result);
    }

    fn make_user(id: u16, name: &str, ip: &str) -> (Arc<server_core::user_pool::AresUser>, tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>) {
        let mut u = server_core::user_pool::AresUser::new(
            id,
            ip.parse().unwrap(),
            [0u8; 16],
        );
        *u.name.write() = name.to_string();
        u.logged_in = true;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        u.sender = Some(tx);
        (Arc::new(u), rx)
    }

    #[test]
    fn pm_object_hybrid_string_and_jspm() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            var pm = __mkPM("hello #world");
            // métodos de string nativos (compat scripts que lo usan como string)
            if (pm.indexOf("hello") !== 0) throw "indexOf";
            if (pm.length !== 12) throw "length: " + pm.length;
            if (pm.toUpperCase() !== "HELLO #WORLD") throw "toUpperCase";
            if (("got " + pm) !== "got hello #world") throw "concat";
            if ((pm == "hello #world") !== true) throw "loose eq";
            // métodos JSPM de sb0t
            if (pm.contains("world") !== true) throw "contains";
            if (pm.contains("xyz") !== false) throw "contains neg";
            if (("" + pm.remove("hello ")) !== '#world') throw "remove";
            if (("" + pm.replace("world", "there")) !== "hello #there") throw "replace";
            if (__mkPM('#scribble#x').isScribble !== true) throw "isScribble";
            if (__mkPM("plain").isScribble !== false) throw "isScribble neg";
        "#,
        );
        assert!(result.is_ok(), "pm hybrid should work: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn users_records_history() {
        let app = make_app();
        // Simular dos desconexiones.
        let (u1, _r1) = make_user(1, "Alice", "10.0.0.1");
        let (u2, _r2) = make_user(2, "Bob", "10.0.0.2");
        app.record_departure(&u1);
        app.record_departure(&u2);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            var recs = Users.records();
            if (recs.length !== 2) throw "expected 2 records, got " + recs.length;
            // más reciente al frente: Bob primero
            if (recs[0].name !== "Bob") throw "recs[0] name: " + recs[0].name;
            if (recs[1].name !== "Alice") throw "recs[1] name: " + recs[1].name;
            if (recs[0].externalIp !== "10.0.0.2") throw "recs[0] ip: " + recs[0].externalIp;
            if (typeof recs[0].guid !== "string" || recs[0].guid.length !== 32) throw "guid hex";
            if (typeof recs[0].ban !== "function") throw "record.ban missing";
            // ban por record devuelve true (persiste)
            if (recs[1].ban() !== true) throw "record.ban should return true";
        "#,
        );
        assert!(result.is_ok(), "records history should work: {:?}", result);
        unregister_context(&ctx);
    }

    #[test]
    fn print_and_send_sb0t_semantics() {
        let app = make_app();
        let (alice, mut rx_a) = make_user(1, "Alice", "10.0.0.1");
        let (bob, mut rx_b) = make_user(2, "Bob", "10.0.0.2");
        app.user_pool.add(alice);
        app.user_pool.add(bob);
        let mut ctx = make_context(app.clone());

        // print(user, texto) → SOLO a ese usuario (mensaje público del bot)
        eval_script(&mut ctx, r#"print(user("Alice"), "hola-alice");"#).unwrap();
        let pa = rx_a.try_recv().expect("Alice recibe print(user, ...)");
        assert_eq!(pa[0], 10, "esperado Public (10)");
        assert!(rx_b.try_recv().is_err(), "Bob NO debe recibir print(user, ...)");

        // print(texto) → a TODOS
        eval_script(&mut ctx, r#"print("hola-todos");"#).unwrap();
        assert_eq!(rx_a.try_recv().expect("Alice broadcast")[0], 10);
        assert_eq!(rx_b.try_recv().expect("Bob broadcast")[0], 10);

        // sendText(user, sender, texto) → a ese usuario
        eval_script(&mut ctx, r#"sendText(user("Bob"), "Bot", "hi-bob");"#).unwrap();
        assert_eq!(rx_b.try_recv().expect("Bob recibe sendText")[0], 10);
        assert!(rx_a.try_recv().is_err(), "Alice NO recibe sendText dirigido a Bob");

        // sendPM(user, sender, texto) → PM a ese usuario
        eval_script(&mut ctx, r#"sendPM(user("Alice"), "Bot", "pm-alice");"#).unwrap();
        assert_eq!(rx_a.try_recv().expect("Alice recibe sendPM")[0], 25, "esperado Pmt (25)");

        // Fallback nativo: sendPublic(from, texto) → broadcast
        eval_script(&mut ctx, r#"sendPublic("Bot", "broadcast-nativo");"#).unwrap();
        assert_eq!(rx_a.try_recv().expect("Alice broadcast nativo")[0], 10);
        assert_eq!(rx_b.try_recv().expect("Bob broadcast nativo")[0], 10);

        unregister_context(&ctx);
    }

    #[test]
    fn user_object_resolves_real_user() {
        let app = make_app();
        let (user, mut rx) = make_user(7, "Alice", "10.1.2.3");
        app.user_pool.add(user);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            var u = user("Alice");
            if (u == null) throw "user() null";
            if (u.name !== "Alice") throw "name != Alice: " + u.name;
            if (u.id !== 7) throw "id != 7: " + u.id;
            if (u.externalIp !== "10.1.2.3") throw "ip bad: " + u.externalIp;
            if (typeof u.level !== "number") throw "level not number";
            // Users.local() debe devolver el array con Alice
            var names = Users.names();
            if (names.indexOf("Alice") < 0) throw "names missing Alice";
            var locals = Users.local();
            if (locals.length !== 1 || locals[0].name !== "Alice") throw "local() bad";
            // sendText: el usuario "dice" el texto en público (paridad
            // sb0t IUser.SendText — mecanismo de #clone).
            if (u.sendText("hola") !== true) throw "sendText not true";
        "#,
        );
        assert!(result.is_ok(), "user object should resolve: {:?}", result);
        let pkt = rx.try_recv().expect("Alice should receive a packet from sendText");
        assert_eq!(pkt[0], 10, "expected Public opcode (10), got {}", pkt[0]);
        let mut r = proto_ares::PacketReader::new(&pkt[1..]);
        assert_eq!(r.read_string_nt().unwrap(), "Alice", "el público sale como el usuario");
        assert_eq!(r.read_string_nt().unwrap(), "hola");
        unregister_context(&ctx);
    }

    #[test]
    fn user_setters_and_new_props() {
        let app = make_app();
        let (alice, _rx) = make_user(1, "Alice", "10.0.0.1");
        app.user_pool.add(alice.clone());

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            var u = user("Alice");
            // Props nuevas
            if (u.visible !== true) throw "visible bad";
            if (u.idle !== false) throw "idle bad";
            if (u.linked !== false) throw "linked bad";
            if (u.localEP.indexOf(":") < 0) throw "localEP bad: " + u.localEP;
            // Setters writable (paridad sb0t)
            u.customName = "Alicia";
            if (u.customName !== "Alicia") throw "customName setter bad: " + u.customName;
            u.muzzled = true;
            if (u.muzzled !== true) throw "muzzled setter bad";
            u.level = 50;
            if (u.level !== 50) throw "level setter bad: " + u.level;
            u.vroom = 3;
            if (u.vroom !== 3) throw "vroom setter bad: " + u.vroom;
            // Métodos stub no revientan
            if (u.getASN() !== null) throw "getASN bad";
            if (u.setUrl("x") !== false) throw "setUrl bad";
            var ig = u.ignores();
            if (!Array.isArray(ig)) throw "ignores bad";
        "#,
        );
        assert!(result.is_ok(), "setters/props: {:?}", result);
        assert_eq!(alice.custom_name.read().clone(), Some("Alicia".to_string()));
        assert!(alice.is_muzzled());
        assert_eq!(*alice.level.read(), server_core::ILevel::Moderator);
        assert_eq!(*alice.vroom.read(), 3);
        unregister_context(&ctx);
    }

    #[test]
    fn send_public_broadcasts() {
        let app = make_app();
        let (user, mut rx) = make_user(1, "Alice", "127.0.0.1");
        app.user_pool.add(user);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"sendPublic("Bot", "hello world");"#,
        );
        assert!(result.is_ok(), "sendPublic should succeed: {:?}", result);

        let pkt = rx.try_recv().expect("user should have received packet");
        assert!(!pkt.is_empty(), "packet should not be empty");
        assert_eq!(pkt[0], 10, "expected Public opcode, got {}", pkt[0]);
    }

    #[test]
    fn send_pm_targets_specific_user() {
        let app = make_app();
        let (user, mut rx) = make_user(1, "Bob", "127.0.0.1");
        app.user_pool.add(user);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            var ok = sendPM("Bot", "Bob", "secret message");
            if (ok !== true) throw "sendPM should return true, got " + ok;
        "#,
        );
        assert!(result.is_ok(), "sendPM should succeed: {:?}", result);

        let pkt = rx.try_recv().expect("Bob should have received packet");
        assert_eq!(pkt[0], 25, "expected Pmt opcode, got {}", pkt[0]);
    }

    #[test]
    fn send_pm_to_nonexistent_returns_false() {
        let app = make_app();
        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            var ok = sendPM("Bot", "Ghost", "boo!");
            if (ok !== false) throw "expected false for missing user, got " + ok;
        "#,
        );
        assert!(result.is_ok(), "sendPM to ghost should return false: {:?}", result);
    }

    #[test]
    fn full_script_flow() {
        // Simula un script completo: define handler, lo invoca, verifica estado.
        let app = make_app();
        let (user, mut rx) = make_user(1, "Carol", "10.0.0.5");
        app.user_pool.add(user);

        let mut ctx = make_context(app);
        let result = eval_script(
            &mut ctx,
            r#"
            function onPublic(from, text) {
                if (text.indexOf("hash ") === 0) {
                    var word = text.substring(5);
                    sendPM("Bot", from, "sha1: " + astraHash(word));
                }
            }
        "#,
        );
        assert!(result.is_ok(), "script should load: {:?}", result);

        let from = JsValue::from(js_string!("Carol"));
        let text = JsValue::from(js_string!("hash hello"));
        call_global_function(&mut ctx, "onPublic", &[from, text]).unwrap();

        let pkt = rx.try_recv().expect("Carol should have received PM");
        assert_eq!(pkt[0], 25, "expected Pmt opcode");
        let payload = String::from_utf8_lossy(&pkt[1..]);
        assert!(payload.contains("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
                "expected SHA-1 of 'hello' in payload, got: {}", payload);
    }

    // ========== Tests Fase 12: File I/O + Zip + ScriptInclude + Spell ==========

    #[test]
    fn file_exists_real() {
        let mut ctx = make_context(make_app());
        // /etc/hostname existe en Linux/macOS
        let result = eval_script(
            &mut ctx,
            r#"
            if (File_exists("/etc/hostname") !== true) {
                throw "expected /etc/hostname to exist";
            }
            if (File_exists("/nonexistent/path/that/does/not/exist") !== false) {
                throw "expected /nonexistent to not exist";
            }
        "#,
        );
        assert!(result.is_ok(), "File_exists should work: {:?}", result);
    }

    #[test]
    fn file_size_real() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const size = File_size("/etc/hostname");
            if (typeof size !== 'number') throw "expected number, got " + typeof size;
            if (size <= 0) throw "expected positive size, got " + size;
        "#,
        );
        assert!(result.is_ok(), "File_size should work: {:?}", result);
    }

    #[test]
    fn file_size_missing_returns_negative() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const size = File_size("/this/does/not/exist");
            if (size !== -1) throw "expected -1 for missing file, got " + size;
        "#,
        );
        assert!(result.is_ok(), "File_size should return -1 on missing: {:?}", result);
    }

    #[test]
    fn zip_compress_decompress_roundtrip() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const original = "Hello, world! This is a test of zip compression.";
            const compressed = Zip_compress(original);
            if (compressed === null) throw "Zip_compress returned null";
            if (typeof compressed !== 'string') throw "expected string, got " + typeof compressed;
            const decompressed = Zip_decompress(compressed);
            if (decompressed !== original) {
                throw "roundtrip failed:\n  original: " + original +
                      "\n  got: " + decompressed;
            }
        "#,
        );
        assert!(result.is_ok(), "Zip roundtrip should work: {:?}", result);
    }

    #[test]
    fn zip_decompress_invalid_returns_null() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const result = Zip_decompress("this is not a zip file");
            if (result !== null) throw "expected null for invalid zip, got " + result;
        "#,
        );
        assert!(result.is_ok(), "invalid zip should return null: {:?}", result);
    }

    #[test]
    fn script_include_runs_other_file() {
        // Crear un archivo temporal con código JS
        let tmp = std::env::temp_dir().join("astra_script_include_test.js");
        std::fs::write(&tmp, "function helper() { return 42; }").unwrap();

        let mut ctx = make_context(make_app());
        let path_str = tmp.to_string_lossy().to_string();
        let script = format!(
            r#"
            const ok = ScriptInclude_run("{}");
            if (ok !== true) throw "ScriptInclude_run returned " + ok;
            if (typeof helper !== 'function') throw "helper not defined after include";
            if (helper() !== 42) throw "helper() did not return 42";
        "#,
            path_str.replace('\\', "\\\\")
        );
        let result = eval_script(&mut ctx, &script);
        let _ = std::fs::remove_file(&tmp);
        assert!(result.is_ok(), "ScriptInclude_run should work: {:?}", result);
    }

    #[test]
    fn script_include_missing_file_returns_false() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = ScriptInclude_run("/this/does/not/exist.js");
            if (ok !== false) throw "expected false for missing file, got " + ok;
        "#,
        );
        assert!(result.is_ok(), "missing file should return false: {:?}", result);
    }

    #[test]
    fn spelling_check_known_word() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Spelling_check("hello") !== true) throw "hello should be known";
            if (Spelling_check("HELLO") !== true) throw "HELLO should be known (case-insensitive)";
            if (Spelling_check("world") !== true) throw "world should be known";
            if (Spelling_check("") !== false) throw "empty string should fail";
        "#,
        );
        assert!(result.is_ok(), "Spelling_check known words: {:?}", result);
    }

    #[test]
    fn spelling_check_unknown_word() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Spelling_check("asdfqwer") !== false) throw "garbage should not be known";
            if (Spelling_check("hello123") !== false) throw "digits should fail";
            if (Spelling_check("hello world") !== false) throw "spaces should fail";
        "#,
        );
        assert!(result.is_ok(), "Spelling_check unknown words: {:?}", result);
    }

    // ========== Tests Fase 13: sb0t-compat aliases + stubs ==========

    #[test]
    fn base64_encode_alias_works() {
        let mut ctx = make_context(make_app());
        // "hello" → "aGVsbG8="
        let result = eval_script(
            &mut ctx,
            r#"
            if (Base64_encode("hello") !== "aGVsbG8=") {
                throw "Base64_encode failed, got: " + Base64_encode("hello");
            }
        "#,
        );
        assert!(result.is_ok(), "Base64_encode should match astraBase64Encode: {:?}", result);
    }

    #[test]
    fn base64_decode_alias_works() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const decoded = Base64_decode("aGVsbG8=");
            if (decoded !== "hello") throw "decode failed, got: " + decoded;
        "#,
        );
        assert!(result.is_ok(), "Base64_decode alias: {:?}", result);
    }

    #[test]
    fn crypto_hash_sha1_alias_works() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const h = Crypto_hashSHA1("hello");
            if (h !== "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d") {
                throw "bad SHA1: " + h;
            }
        "#,
        );
        assert!(result.is_ok(), "Crypto_hashSHA1 alias: {:?}", result);
    }

    #[test]
    fn crypto_hash_md5_alias_works() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const h = Crypto_hashMD5("hello");
            if (h !== "5d41402abc4b2a76b9719d911017c592") {
                throw "bad MD5: " + h;
            }
        "#,
        );
        assert!(result.is_ok(), "Crypto_hashMD5 alias: {:?}", result);
    }

    #[test]
    fn users_count_alias_works() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Users_count() !== userCount()) {
                throw "Users_count should equal userCount, got " + Users_count() + " vs " + userCount();
            }
        "#,
        );
        assert!(result.is_ok(), "Users_count alias: {:?}", result);
    }

    #[test]
    fn room_set_topic_alias_works() {
        let app = make_app();
        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            Room_setTopic("via alias");
            if (getTopic() !== "via alias") throw "Room_setTopic didn't work";
        "#,
        );
        assert!(result.is_ok(), "Room_setTopic alias: {:?}", result);
    }

    #[test]
    fn channels_list_returns_array() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const list = Channels_list();
            if (typeof list !== 'string') throw "expected string (JSON), got " + typeof list;
            if (list !== "[0]") throw "expected '[0]', got " + list;
        "#,
        );
        assert!(result.is_ok(), "Channels_list: {:?}", result);
    }

    #[test]
    fn hashlink_create_formats_url() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const url = Hashlink_create("server.com", 5009);
            if (url !== "astrahash://server.com:5009") {
                throw "bad URL: " + url;
            }
        "#,
        );
        assert!(result.is_ok(), "Hashlink_create: {:?}", result);
    }

    #[test]
    fn users_get_by_name_returns_null_for_missing() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const u = Users_getUserByName("Ghost");
            if (u !== null) throw "expected null for missing user, got " + u;
        "#,
        );
        assert!(result.is_ok(), "Users_getUserByName missing: {:?}", result);
    }

    #[test]
    fn users_get_by_name_returns_info_for_existing() {
        let app = make_app();
        let (user, _rx) = make_user(1, "Alice", "10.0.0.5");
        app.user_pool.add(user);
        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            const u = Users_getUserByName("Alice");
            if (typeof u !== 'string') throw "expected string, got " + typeof u;
            if (u.indexOf("Alice") === -1) throw "missing name in: " + u;
        "#,
        );
        assert!(result.is_ok(), "Users_getUserByName existing: {:?}", result);
    }

    #[test]
    fn stats_add_and_get_roundtrip() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            Stats_addStat("joins", 42);
            Stats_addStat("parts", 7);
            if (Stats_getStat("joins") !== 42) throw "expected 42, got " + Stats_getStat("joins");
            if (Stats_getStat("parts") !== 7) throw "expected 7, got " + Stats_getStat("parts");
            if (Stats_getStat("missing") !== 0) throw "expected 0 for missing";
        "#,
        );
        assert!(result.is_ok(), "Stats roundtrip: {:?}", result);
    }

    #[test]
    fn stats_overwrite_replaces_value() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            Stats_addStat("k", 1);
            Stats_addStat("k", 2);
            if (Stats_getStat("k") !== 2) throw "overwrite failed, got " + Stats_getStat("k");
        "#,
        );
        assert!(result.is_ok(), "Stats overwrite: {:?}", result);
    }

    #[test]
    fn entities_list_returns_empty_array() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Entities_list() !== "[]") throw "expected '[]', got " + Entities_list();
        "#,
        );
        assert!(result.is_ok(), "Entities_list: {:?}", result);
    }

    #[test]
    fn link_create_link_is_stub() {
        let mut ctx = make_context(make_app());
        // Por ahora retorna false y loguea warning. Tests de "no implementado" honesto.
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Link_createLink("hub.example.com", 5009);
            if (ok !== false) throw "expected false (not implemented), got " + ok;
        "#,
        );
        assert!(result.is_ok(), "Link_createLink stub: {:?}", result);
    }

    #[test]
    fn registry_create_and_delete_key() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const path = Registry_createKey("MyApp");
            if (path !== "HKLM\\Software\\Astra\\MyApp") {
                throw "bad path: " + path;
            }
            // Borrarla retorna true
            const removed = Registry_deleteKey("MyApp");
            if (removed !== true) throw "delete should return true, got " + removed;
            // Borrar de nuevo retorna false
            const removed2 = Registry_deleteKey("MyApp");
            if (removed2 !== false) throw "second delete should return false, got " + removed2;
        "#,
        );
        assert!(result.is_ok(), "Registry create/delete: {:?}", result);
    }

    #[test]
    fn room_broadcast_sends_to_all() {
        let app = make_app();
        let (user1, mut rx1) = make_user(1, "Alice", "127.0.0.1");
        let (user2, mut rx2) = make_user(2, "Bob", "127.0.0.2");
        app.user_pool.add(user1);
        app.user_pool.add(user2);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Room_broadcast("hello from bot");
            if (ok !== true) throw "Room_broadcast should return true, got " + ok;
        "#,
        );
        assert!(result.is_ok(), "Room_broadcast: {:?}", result);

        // Ambos users deben recibir el mensaje
        let p1 = rx1.try_recv().expect("Alice should receive");
        let p2 = rx2.try_recv().expect("Bob should receive");
        assert_eq!(p1[0], 10, "Alice: expected Public opcode");
        assert_eq!(p2[0], 10, "Bob: expected Public opcode");
    }

    // ========== Tests Fase 16: Channels_* (Vroom) ==========

    #[test]
    fn channels_list_includes_vroom_0() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const list = Channels_list();
            if (typeof list !== 'string') throw "expected string, got " + typeof list;
            if (list !== "[0]") throw "expected '[0]', got " + list;
        "#,
        );
        assert!(result.is_ok(), "Channels_list: {:?}", result);
    }

    #[test]
    fn channels_create_and_list() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok1 = Channels_create(1, "Sala 1");
            if (ok1 !== true) throw "create 1 should be true, got " + ok1;
            const ok2 = Channels_create(2, "Sala 2");
            if (ok2 !== true) throw "create 2 should be true, got " + ok2;
            // Duplicado debe fallar
            const ok3 = Channels_create(1, "Sala 1 dup");
            if (ok3 !== false) throw "create dup should be false, got " + ok3;
            // Lista debe tener 0, 1, 2
            const list = Channels_list();
            if (list !== "[0,1,2]") throw "expected '[0,1,2]', got " + list;
        "#,
        );
        assert!(result.is_ok(), "Channels_create: {:?}", result);
    }

    #[test]
    fn channels_get_returns_json() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const info = Channels_get(0);
            if (typeof info !== 'string') throw "expected string, got " + typeof info;
            if (info.indexOf("\"id\":0") === -1) throw "missing id: " + info;
            if (info.indexOf("\"name\":\"Main Room\"") === -1) throw "missing name: " + info;
            // vroom inexistente → null
            const none = Channels_get(99);
            if (none !== "null") throw "expected null for missing, got " + none;
        "#,
        );
        assert!(result.is_ok(), "Channels_get: {:?}", result);
    }

    #[test]
    fn channels_set_topic() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Channels_setTopic(0, "nuevo topic");
            if (ok !== true) throw "setTopic should return true";
            const info = Channels_get(0);
            if (info.indexOf("nuevo topic") === -1) throw "topic not updated: " + info;
            // vroom inexistente → false
            const ok2 = Channels_setTopic(99, "x");
            if (ok2 !== false) throw "setTopic on missing should return false";
        "#,
        );
        assert!(result.is_ok(), "Channels_setTopic: {:?}", result);
    }

    #[test]
    fn channels_broadcast_only_to_vroom() {
        // Crear 2 users en vroom 0 y 1 user en vroom 1
        let app = make_app();
        let (alice, mut rx_alice) = make_user(1, "Alice", "127.0.0.1");
        *alice.vroom.write() = 0;
        let (bob, mut rx_bob) = make_user(2, "Bob", "127.0.0.2");
        *bob.vroom.write() = 0;
        let (charlie, mut rx_charlie) = make_user(3, "Charlie", "127.0.0.3");
        *charlie.vroom.write() = 1;
        app.user_pool.add(alice);
        app.user_pool.add(bob);
        app.user_pool.add(charlie);
        // Crear vroom 1
        app.vrooms.create(1, Some("Sala 1".into()), None);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            // Broadcast en vroom 0 → solo Alice y Bob
            const ok = Channels_broadcast(0, "Bot", "hola v0");
            if (ok !== true) throw "broadcast v0 should return true";
            // Broadcast en vroom 1 → solo Charlie
            const ok2 = Channels_broadcast(1, "Bot", "hola v1");
            if (ok2 !== true) throw "broadcast v1 should return true";
            // Broadcast en vroom 99 (no existe, sin users) → false
            const ok3 = Channels_broadcast(99, "Bot", "nadie");
            if (ok3 !== false) throw "broadcast v99 should return false";
        "#,
        );
        assert!(result.is_ok(), "Channels_broadcast: {:?}", result);

        // Verificar
        assert!(rx_alice.try_recv().is_ok(), "Alice debería recibir v0");
        assert!(rx_bob.try_recv().is_ok(), "Bob debería recibir v0");
        assert!(rx_charlie.try_recv().is_ok(), "Charlie debería recibir v1");
        // Charlie no debería recibir v0
        assert!(rx_charlie.try_recv().is_err(), "Charlie NO debería recibir v0");
    }

    // ========== Tests Fase 17: Hashlink + Link_* ==========

    #[test]
    fn hashlink_parse_valid() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const r = Hashlink_parse("astrahash://server.com:5009");
            if (typeof r !== 'string') throw "expected string, got " + typeof r;
            if (r.indexOf("\"server\":\"server.com\"") === -1) throw "missing server: " + r;
            if (r.indexOf("\"port\":5009") === -1) throw "missing port: " + r;
        "#,
        );
        assert!(result.is_ok(), "Hashlink_parse valid: {:?}", result);
    }

    #[test]
    fn hashlink_parse_invalid_returns_null() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Hashlink_parse("not a hashlink") !== null) throw "expected null for invalid";
            if (Hashlink_parse("astrahash://server:invalid") !== null) throw "expected null for bad port";
            if (Hashlink_parse("astrahash://:5009") !== null) throw "expected null for empty server";
            if (Hashlink_parse("astrahash://server.com") !== null) throw "expected null for missing port";
        "#,
        );
        assert!(result.is_ok(), "Hashlink_parse invalid: {:?}", result);
    }

    #[test]
    fn link_list_empty_by_default() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (Link_list() !== "[]") throw "expected '[]', got " + Link_list();
        "#,
        );
        assert!(result.is_ok(), "Link_list: {:?}", result);
    }

    #[test]
    fn link_get_user_list_local_only() {
        let app = make_app();
        let (user1, _rx) = make_user(1, "Alice", "127.0.0.1");
        let (user2, _rx) = make_user(2, "Bob", "127.0.0.2");
        app.user_pool.add(user1);
        app.user_pool.add(user2);

        let mut ctx = make_context(app.clone());
        let result = eval_script(
            &mut ctx,
            r#"
            const list = Link_getUserList();
            if (typeof list !== 'string') throw "expected string, got " + typeof list;
            if (list.indexOf("Alice") === -1) throw "missing Alice in: " + list;
            if (list.indexOf("Bob") === -1) throw "missing Bob in: " + list;
        "#,
        );
        assert!(result.is_ok(), "Link_getUserList: {:?}", result);
    }

    #[test]
    fn link_create_link_returns_false_stub() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Link_createLink("hub.example.com", 5009);
            if (ok !== false) throw "Link_createLink should be stub returning false, got " + ok;
        "#,
        );
        assert!(result.is_ok(), "Link_createLink stub: {:?}", result);
    }

    #[test]
    fn link_disconnect_returns_false_stub() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Link_disconnect("hub.example.com");
            if (ok !== false) throw "Link_disconnect should be stub returning false";
        "#,
        );
        assert!(result.is_ok(), "Link_disconnect stub: {:?}", result);
    }

    // ========== Tests Fase 18: Avatar_new + Avatar_getSize ==========

    #[test]
    fn avatar_new_returns_id() {
        let mut ctx = make_context(make_app());
        // "hello" en base64 = "aGVsbG8="
        let result = eval_script(
            &mut ctx,
            r#"
            const id = Avatar_new("aGVsbG8=");
            if (typeof id !== 'number') throw "expected number, got " + typeof id;
            if (id < 0) throw "expected id >= 0, got " + id;
        "#,
        );
        assert!(result.is_ok(), "Avatar_new: {:?}", result);
    }

    #[test]
    fn avatar_new_invalid_base64_returns_negative() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const id = Avatar_new("not base64 @@@");
            if (id !== -1) throw "expected -1 for invalid base64, got " + id;
        "#,
        );
        assert!(result.is_ok(), "Avatar_new invalid: {:?}", result);
    }

    #[test]
    fn avatar_get_size_returns_correct_value() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const id = Avatar_new("aGVsbG8=");  // 5 bytes ("hello")
            const size = Avatar_getSize(id);
            if (size !== 5) throw "expected size 5, got " + size;
            // id inválido → -1
            const bad = Avatar_getSize(-5);
            if (bad !== -1) throw "expected -1 for invalid id, got " + bad;
            // id fuera de rango → -1
            const oob = Avatar_getSize(99999);
            if (oob !== -1) throw "expected -1 for OOB, got " + oob;
        "#,
        );
        assert!(result.is_ok(), "Avatar_getSize: {:?}", result);
    }

    // ========== Tests Fase 19: Spelling_suggest + Query_* ==========

    #[test]
    fn spelling_suggest_returns_array() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            // "helo" no está en el dict pero comparte prefijo "he" con "hello"
            const arr = Spelling_suggest("helo");
            if (typeof arr !== 'string') throw "expected string, got " + typeof arr;
            if (arr.indexOf("hello") === -1) throw "expected 'hello' suggestion, got " + arr;
        "#,
        );
        assert!(result.is_ok(), "Spelling_suggest: {:?}", result);
    }

    #[test]
    fn spelling_suggest_no_match() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            // "xyz" no tiene prefijo con ninguna palabra del dict
            const arr = Spelling_suggest("xyz");
            if (arr !== "[]") throw "expected '[]', got " + arr;
        "#,
        );
        assert!(result.is_ok(), "Spelling_suggest no match: {:?}", result);
    }

    #[test]
    fn query_new_select_works() {
        let app = make_app();
        // Insertar datos de prueba via método público
        app.db.execute(
            "INSERT INTO bans (name, version, guid, externalip, localip, port, ident) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params!["TestUser", "1.0", "guid1234", "1.2.3.4", "1.2.3.4", 5009i64, 1i64],
        ).unwrap();
        let mut ctx = make_context(app);
        let result = eval_script(
            &mut ctx,
            r#"
            const id = Query_new("SELECT name, version FROM bans WHERE ident = 1");
            if (typeof id !== 'number') throw "expected number, got " + typeof id;
            if (id < 0) throw "expected id >= 0, got " + id;
            const results = Query_getResults(id);
            if (typeof results !== 'string') throw "expected string";
            if (results.indexOf("TestUser") === -1) throw "expected TestUser in results: " + results;
            if (results.indexOf("\"version\":\"1.0\"") === -1) throw "expected version 1.0: " + results;
            const cols = Query_getColumnCount(id);
            if (cols !== 2) throw "expected 2 columns, got " + cols;
            const rows = Query_getRowCount(id);
            if (rows !== 1) throw "expected 1 row, got " + rows;
        "#,
        );
        assert!(result.is_ok(), "Query_new SELECT: {:?}", result);
    }

    #[test]
    fn query_new_blocks_writes() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const id = Query_new("DELETE FROM bans");
            if (id !== -1) throw "DELETE should be blocked, got id " + id;
            const id2 = Query_new("DROP TABLE bans");
            if (id2 !== -1) throw "DROP should be blocked, got id " + id2;
            const id3 = Query_new("INSERT INTO bans VALUES (...)");
            if (id3 !== -1) throw "INSERT should be blocked, got id " + id3;
        "#,
        );
        assert!(result.is_ok(), "Query_new blocks writes: {:?}", result);
    }

    #[test]
    fn query_get_results_nonexistent_returns_null() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const r = Query_getResults(99999);
            if (r !== "null") throw "expected null, got " + r;
        "#,
        );
        assert!(result.is_ok(), "Query_getResults null: {:?}", result);
    }

    // ========== Tests Fase 20: Help_addLine + setTimer + clearTimer ==========

    #[test]
    fn help_add_line_returns_true() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const ok = Help_addLine("hola", "saluda al bot");
            if (ok !== true) throw "expected true, got " + ok;
            // Línea vacía → false
            const ok2 = Help_addLine("", "");
            if (ok2 !== false) throw "expected false for empty, got " + ok2;
        "#,
        );
        assert!(result.is_ok(), "Help_addLine: {:?}", result);
    }

    #[test]
    fn help_add_line_persists_for_handle_help() {
        // Verificar que la línea agregada se pueda recuperar vía extra_help_lines()
        let mut ctx = make_context(make_app());
        eval_script(
            &mut ctx,
            r#"Help_addLine("test_cmd", "comando de prueba");"#,
        )
        .unwrap();
        let lines = extra_help_lines();
        let found: Vec<_> = lines
            .iter()
            .filter(|(c, _)| c == "test_cmd")
            .collect();
        assert_eq!(found.len(), 1, "expected 1 line for test_cmd, got {}", found.len());
        assert!(found[0].1.contains("comando de prueba"));
    }

    #[test]
    fn set_timer_returns_id() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            const id = setTimer(60, "myCallback");
            if (typeof id !== 'number') throw "expected number, got " + typeof id;
            if (id <= 0) throw "expected positive id, got " + id;
            // clearTimer
            const cleared = clearTimer(id);
            if (cleared !== true) throw "clearTimer should return true, got " + cleared;
            // clearTimer de id no existente → false
            const cleared2 = clearTimer(99999);
            if (cleared2 !== false) throw "expected false for missing id, got " + cleared2;
        "#,
        );
        assert!(result.is_ok(), "setTimer: {:?}", result);
    }

    #[test]
    fn set_timer_invalid_args() {
        let mut ctx = make_context(make_app());
        let result = eval_script(
            &mut ctx,
            r#"
            if (setTimer(0, "cb") !== -1) throw "expected -1 for secs=0";
            if (setTimer(60, "") !== -1) throw "expected -1 for empty fn";
        "#,
        );
        assert!(result.is_ok(), "setTimer invalid: {:?}", result);
    }
}

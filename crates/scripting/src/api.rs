//! API expuesta a los scripts JS.
//!
//! Las native functions NO capturan estado en el closure (limitación de
//! `boa_engine 0.20` que requiere `Copy` para capturar). En su lugar,
//! usan un **registro global** que mapea `Context* → Arc<AppContext>`.
//! Cuando `make_context` se llama, registra el `Arc<AppContext>`.
//! Cuando la `Context` se destruye, hay que llamar `unregister_context`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use boa_engine::{
    context::Context,
    js_string,
    native_function::NativeFunction,
    value::JsValue,
};

use server_core::AppContext;

// ============================================================================
// Registry global: Context* → Arc<AppContext>
// ============================================================================

type Registry = Mutex<HashMap<usize, Weak<AppContext>>>;

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registra un `Arc<AppContext>` para un `Context`.
pub fn register_context(ctx: &Context, app: &Arc<AppContext>) {
    let key = ctx as *const Context as usize;
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, Arc::downgrade(app));
}

/// Elimina el registro de un `Context`.
pub fn unregister_context(ctx: &Context) {
    let key = ctx as *const Context as usize;
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key);
}

/// Obtiene el `Arc<AppContext>` registrado para un `Context`.
fn lookup_app(ctx: &Context) -> Option<Arc<AppContext>> {
    let key = ctx as *const Context as usize;
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .and_then(|w| w.upgrade())
}

// ============================================================================
// make_context
// ============================================================================

/// Crea un `Context` JS con el API de `astra.*` inyectada.
///
/// `app` es el `AppContext` del servidor, que se registrará automáticamente
/// para que los native functions puedan acceder a él.
pub fn make_context(app: Arc<AppContext>) -> Context {
    let mut context = Context::default();

    // Registramos el app en el registry para que las native functions
    // puedan acceder a él vía &Context.
    register_context(&context, &app);

    // print(msg)
    let print_fn = NativeFunction::from_fn_ptr(|_this, args, _ctx| {
        let mut msg = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                msg.push(' ');
            }
            msg.push_str(&format_js_value(arg));
        }
        tracing::info!("[script] print: {}", msg);
        Ok(JsValue::undefined())
    });
    context
        .register_global_builtin_callable(js_string!("print"), 1, print_fn)
        .expect("print should be registered");

    // astra.log(msg)
    let log_fn = NativeFunction::from_fn_ptr(|_this, args, _ctx| {
        let msg = args.get(0).map(format_js_value).unwrap_or_default();
        tracing::info!("[script] log: {}", msg);
        Ok(JsValue::undefined())
    });
    context
        .register_global_builtin_callable(js_string!("log"), 1, log_fn)
        .expect("log should be registered");

    // astra.userCount() → número real de usuarios
    let user_count_fn = NativeFunction::from_fn_ptr(|_this, _args, ctx| {
        if let Some(app) = lookup_app(ctx) {
            Ok(JsValue::from(app.user_pool.len() as i32))
        } else {
            Ok(JsValue::from(0))
        }
    });
    context
        .register_global_builtin_callable(js_string!("userCount"), 0, user_count_fn)
        .expect("userCount should be registered");

    context
}

/// Convierte un `JsValue` a string.
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

/// Evalúa código fuente JS en el contexto.
pub fn eval_script(ctx: &mut Context, source: &str) -> Result<(), String> {
    ctx.eval(boa_engine::Source::from_bytes(source.as_bytes()))
        .map_err(|e| format!("eval error: {}", e))?;
    Ok(())
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
        // Sin usuarios → userCount() == 0
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
        // No registrado
        assert!(lookup_app(&ctx).is_none());
        // Registrar
        register_context(&ctx, &app);
        let found = lookup_app(&ctx);
        assert!(found.is_some());
        assert_eq!(found.unwrap().settings.port, app.settings.port);
        // Desregistrar
        unregister_context(&ctx);
        assert!(lookup_app(&ctx).is_none());
    }
}

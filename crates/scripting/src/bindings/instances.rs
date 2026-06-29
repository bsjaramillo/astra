//! Instances (constructores de instancias con métodos específicos).
//!
//! En el sb0t original, los "Instance" son las clases que se usan
//! para los callbacks (ej. JSAvatarInstance tiene el avatar real
//! que se guarda en el userpool).

use std::sync::Arc;
use boa_engine::{Context, JsValue};

use super::super::ScriptState;
use super::register_fn;

pub fn register(ctx: &mut Context, _state: Arc<ScriptState>) {
    register_fn(ctx, "JSAvatarInstance_new", 2, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let hash = args.get(1).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{name:\"{}\",hash:\"{}\"}}", name, hash)))
    });
    register_fn(ctx, "JSAvatarInstance_setHash", 2, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSAvatarInstance_clear", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSAvatarInstance_scale", 1, |_this, _args, _ctx| {
        Ok(JsValue::undefined())
    });
    register_fn(ctx, "JSHttpRequestInstance_new", 1, |_this, args, _ctx| {
        let url = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("HttpRequest:{}", url)))
    });
    register_fn(ctx, "JSHttpRequestInstance_send", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSHttpRequestInstance_abort", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSListInstance_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSListInstance_add", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSListInstance_remove", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSListInstance_count", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(0))
    });
    register_fn(ctx, "JSListInstance_item", 1, |_this, args, _ctx| {
        Ok(JsValue::undefined())
    });
    register_fn(ctx, "JSProxyCheckInstance_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSProxyCheckInstance_query", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(false))
    });
    register_fn(ctx, "JSQueryInstance_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from("Query"))
    });
    register_fn(ctx, "JSQueryInstance_getCount", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(0))
    });
    register_fn(ctx, "JSQueryInstance_getNext", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSScribbleInstance_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSScribbleInstance_setDimensions", 2, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSScribbleInstance_toBytes", 0, |_this, _args, _ctx| {
        Ok(JsValue::undefined())
    });
    register_fn(ctx, "JSSqlInstance_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSSqlInstance_query", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSTimerInstance_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSTimerInstance_stop", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSTimerInstance_start", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "JSXmlParserInstance_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "JSXmlParserInstance_parse", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
}

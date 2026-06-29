//! Properties (helpers de clase) del API JS.
//!
//! En el sb0t original, estos son "Property helpers" — métodos estáticos
//! que devuelven objetos. En Astra, los exponemos como funciones globales.

use std::sync::Arc;
use boa_engine::{js_string, Context, JsValue, NativeFunction};

use super::super::ScriptState;
use super::register_fn;

pub fn register(ctx: &mut Context, _state: Arc<ScriptState>) {
    // Commands.list()
    register_fn(ctx, "Commands_list", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // Channels.get(name)
    register_fn(ctx, "Channels_get", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("Channel:{}", name)))
    });
    // Channels.list()
    register_fn(ctx, "Channels_list2", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // Hashlink.parse(url)
    register_fn(ctx, "Hashlink_parse", 1, |_this, args, _ctx| {
        let url = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        // Devuelve un hashlink parseado (stub)
        Ok(JsValue::from(format!("{{server:\"127.0.0.1\",port:5009,hash:\"{}\"}}", url)))
    });
    // Hashlink.create(server, port)
    register_fn(ctx, "Hashlink_create2", 2, |_this, args, _ctx| {
        let server = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let port = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        Ok(JsValue::from(format!("astrahash://{}:{}", server, port)))
    });
    // Link.list() (compatible con el original)
    register_fn(ctx, "Link_list2", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // Link.connect(server, port)
    register_fn(ctx, "Link_connect", 2, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    // Link.disconnect(name)
    register_fn(ctx, "Link_disconnect", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    // Link.findLeaf(name)
    register_fn(ctx, "Link_findLeaf", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("Leaf:{}", name)))
    });
    // Link.findUser(name, leaf?)
    register_fn(ctx, "Link_findUser", 2, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("User:{}", name)))
    });
    // Link.findHub(name)
    register_fn(ctx, "Link_findHub", 1, |_this, _args, _ctx| {
        Ok(JsValue::undefined())
    });
    // Link.kickHub(name)
    register_fn(ctx, "Link_kickHub", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(true))
    });
    // Link.getUserList(leaf?)
    register_fn(ctx, "Link_getUserList", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
}

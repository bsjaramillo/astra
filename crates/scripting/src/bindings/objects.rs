//! Objects (constructores de clases) del API JS.
//!
//! Cada función `Xxx_new(...)` corresponde al constructor `new Xxx(...)`
//! en el sb0t original. Devuelve un objeto con métodos básicos.

use std::sync::Arc;
use boa_engine::{js_string, Context, JsValue, NativeFunction};

use super::super::ScriptState;
use super::register_fn;

pub fn register(ctx: &mut Context, _state: Arc<ScriptState>) {
    // new User(name, level, flags, vroom, port, file_count, age, sex, country, region, custom_name, personal_message)
    register_fn(ctx, "User_new", 13, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!(
            "{{name:\"{}\",level:1,flags:0,vroom:0,port:0,fileCount:0,age:0,sex:0,country:0,region:\"\",customName:\"\",personalMessage:\"\"}}",
            name
        )))
    });
    // new Channel(name, topic, count, password, language, motd, min_version, max_version, banlist, ...)
    register_fn(ctx, "Channel_new", 10, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let topic = args.get(1).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!(
            "{{name:\"{}\",topic:\"{}\",count:0,password:\"\",language:0,motd:\"\",minVersion:0,maxVersion:0,banlist:[]}}",
            name, topic
        )))
    });
    // new Avatar(name, hash, is_default)
    register_fn(ctx, "Avatar_new", 3, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!(
            "{{name:\"{}\",hash:\"\",isDefault:false}}",
            name
        )))
    });
    // new BannedUser(name, ip, by, time, reason)
    register_fn(ctx, "BannedUser_new", 5, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let reason = args.get(4).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!(
            "{{name:\"{}\",ip:\"\",by:\"\",time:0,reason:\"{}\"}}",
            name, reason
        )))
    });
    // new PM(from, to, text)
    register_fn(ctx, "PM_new", 3, |_this, args, _ctx| {
        let from = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let to = args.get(1).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let text = args.get(2).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{from:\"{}\",to:\"{}\",text:\"{}\"}}", from, to, text)))
    });
    // new HashlinkResult(server, port, name, topic, count, hash)
    register_fn(ctx, "HashlinkResult_new", 6, |_this, args, _ctx| {
        let server = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let port = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        let name = args.get(2).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let topic = args.get(3).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let count = args.get(4).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        Ok(JsValue::from(format!(
            "{{server:\"{}\",port:{},name:\"{}\",topic:\"{}\",count:{},hash:\"\"}}",
            server, port, name, topic, count
        )))
    });
    // new Node(server, port, file_count, name, topic, count, ident)
    register_fn(ctx, "Node_new", 7, |_this, args, _ctx| {
        let server = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let port = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        Ok(JsValue::from(format!("{{server:\"{}\",port:{},name:\"\",topic:\"\",count:0,ident:0}}", server, port)))
    });
    // new Leaf(server, port, name, ident)
    register_fn(ctx, "Leaf_new", 4, |_this, args, _ctx| {
        let server = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let port = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        Ok(JsValue::from(format!("{{server:\"{}\",port:{},name:\"\",ident:0}}", server, port)))
    });
    // new List()
    register_fn(ctx, "List_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new Record()
    register_fn(ctx, "Record_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new HttpRequestResult(success, status, statusText, body)
    register_fn(ctx, "HttpRequestResult_new", 4, |_this, args, _ctx| {
        let success = args.get(0).and_then(|v| v.as_boolean()).unwrap_or(false);
        let status = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        let body = args.get(3).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{success:{},status:{},statusText:\"\",body:\"{}\"}}", success, status, body)))
    });
    // new IgnoreCollection()
    register_fn(ctx, "IgnoreCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new ChannelCollection()
    register_fn(ctx, "ChannelCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new AvatarImage(name, hash)
    register_fn(ctx, "AvatarImage_new", 2, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{name:\"{}\",hash:\"\"}}", name)))
    });
    // new NodeCollection()
    register_fn(ctx, "NodeCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new NodeAttributes(flag, x, y, ...)
    register_fn(ctx, "NodeAttributes_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from("{}"))
    });
    // new ScribbleImage(name, hash, height)
    register_fn(ctx, "ScribbleImage_new", 3, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{name:\"{}\",hash:\"\",height:0}}", name)))
    });
    // new SpellingSuggestionCollection()
    register_fn(ctx, "SpellingSuggestionCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new CryptoResult(hash, type)
    register_fn(ctx, "CryptoResult_new", 2, |_this, args, _ctx| {
        let hash = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let kind = args.get(1).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("{{hash:\"{}\",type:\"{}\"}}", hash, kind)))
    });
    // new RegistryKeyCollection()
    register_fn(ctx, "RegistryKeyCollection_new", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new ProxyCheckResult(isProxy, level)
    register_fn(ctx, "ProxyCheckResult_new", 2, |_this, args, _ctx| {
        let is_proxy = args.get(0).and_then(|v| v.as_boolean()).unwrap_or(false);
        let level = args.get(1).and_then(|v| v.as_number()).map(|n| n as u8).unwrap_or(0);
        Ok(JsValue::from(format!("{{isProxy:{},level:{}}}", is_proxy, level)))
    });
    // new Query(sql)
    register_fn(ctx, "Query_new", 1, |_this, args, _ctx| {
        let sql = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("Query:{}", sql)))
    });
    // new Sql(connection, query, callback)
    register_fn(ctx, "Sql_new", 3, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    // new XmlParser(data)
    register_fn(ctx, "XmlParser_new", 1, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
}

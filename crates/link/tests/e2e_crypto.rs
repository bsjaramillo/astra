//! Test E2E del link encriptado (paridad sb0t).
//!
//! Levanta un hub y un leaf reales sobre TCP loopback, con la lista de
//! trusted leaves configurada, y verifica que:
//! 1. El handshake cifrado se completa (credentials válidas → HubAck con
//!    key AES ofuscada → leaf deriva la key).
//! 2. La userlist del hub llega al leaf con los strings descifrados
//!    correctamente (prueba el AES-256-CBC de extremo a extremo).

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use server_core::db::Database;
use server_core::settings::{Settings, TrustedLeaf};
use server_core::{AppContext, AresUser};

const LEAF_ROOM: &str = "LeafRoom";
const LEAF_GUID: &str = "leaf-guid-1234567890";

fn make_ctx(room_name: &str, guid: &str, trusted: Vec<TrustedLeaf>) -> Arc<AppContext> {
    let mut settings = Settings::default();
    settings.room_name = room_name.to_string();
    settings.guid = guid.to_string();
    settings.link_trusted_leaves = trusted;
    let db = Database::in_memory().unwrap();
    Arc::new(AppContext::new(settings, db))
}

fn add_user(ctx: &AppContext, id: u16, name: &str) {
    let mut user = AresUser::new(id, IpAddr::V4(Ipv4Addr::new(10, 0, 0, id as u8)), [id as u8; 16]);
    user.logged_in = true;
    user.version = "Ares 2.5".to_string();
    *user.name.write() = name.to_string();
    ctx.user_pool.add(Arc::new(user));
}

#[tokio::test]
async fn encrypted_link_handshake_and_userlist() {
    // Hub confía en el leaf (name + guid del leaf).
    let hub = make_ctx(
        "HubRoom",
        "hub-guid-abcdef",
        vec![TrustedLeaf {
            name: LEAF_ROOM.to_string(),
            guid: LEAF_GUID.to_string(),
        }],
    );
    add_user(&hub, 1, "HubUser");

    let leaf = make_ctx(LEAF_ROOM, LEAF_GUID, Vec::new());
    add_user(&leaf, 2, "LeafUser");

    // Listener del hub en un puerto efímero.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let hub_app = hub.clone();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let _ = astra_link::handle_stream(hub_app, stream).await;
        }
    });

    // Arrancar el leaf apuntando al hub.
    let client = Arc::new(astra_link::LinkClient::new(leaf.clone()));
    let run_client = client.clone();
    tokio::spawn(async move {
        run_client.run(addr).await;
    });

    // Esperar a que el leaf reciba la userlist del hub (descifrada).
    let mut got_hub_user = false;
    for _ in 0..50 {
        if client.peer_users().iter().any(|u| u.name == "HubUser") {
            got_hub_user = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        got_hub_user,
        "el leaf debía recibir 'HubUser' del hub con los strings descifrados; \
         peer_users = {:?}",
        client.peer_users().iter().map(|u| u.name.clone()).collect::<Vec<_>>()
    );

    client.close();
}

#[tokio::test]
async fn untrusted_leaf_is_rejected() {
    // Hub confía en un leaf con guid distinto → el leaf real no matchea.
    let hub = make_ctx(
        "HubRoom",
        "hub-guid-abcdefg",
        vec![TrustedLeaf {
            name: LEAF_ROOM.to_string(),
            guid: "otro-guid-diferente".to_string(),
        }],
    );

    let leaf = make_ctx(LEAF_ROOM, LEAF_GUID, Vec::new());
    add_user(&leaf, 2, "LeafUser");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let hub_app = hub.clone();
    let hub_handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            // El hub debe rechazar por credentials no coincidentes.
            astra_link::handle_stream(hub_app, stream).await
        } else {
            Ok(())
        }
    });

    let client = Arc::new(astra_link::LinkClient::new(leaf.clone()));
    let run_client = client.clone();
    tokio::spawn(async move {
        run_client.run(addr).await;
    });

    // El hub debe terminar con Err (leaf no autorizado).
    let result = tokio::time::timeout(Duration::from_secs(3), hub_handle).await;
    client.close();
    match result {
        Ok(Ok(inner)) => assert!(inner.is_err(), "el hub debía rechazar al leaf no autorizado"),
        Ok(Err(_join_err)) => {}
        Err(_) => panic!("timeout: el hub no resolvió el handshake"),
    }
}

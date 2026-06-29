//! Enums de mensajes del protocolo Ares (TCP y UDP).
//!
//! Mapeo 1:1 desde `core/TCPMsg.cs` y `core/Udp/UdpMsg.cs` del proyecto sb0t original.

use std::convert::TryFrom;

/// Mensajes del protocolo TCP Ares (ver `TCPMsg.cs`).
///
/// El número del mensaje es el primer byte del payload TCP.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TcpMsg {
    /// Error genérico del servidor
    ServerError = 0,
    /// El cliente solicita re-login (reconexión)
    ClientRelogin = 1,
    /// Login del cliente
    ClientLogin = 2,
    /// ACK del servidor al login
    ServerLoginAck = 3,
    /// Update de status del cliente
    ClientUpdateStatus = 4,
    /// Update de status broadcast del servidor
    ServerUpdateUserStatus = 5,
    /// Redirección a otra sala
    ServerRedirect = 6,
    /// Auto-login (con credenciales guardadas)
    ClientAutologin = 7,
    /// Echo del servidor
    ServerEcho = 8,
    /// Avatar (cliente y servidor usan el mismo opcode)
    Avatar = 9,
    /// Mensaje público (cliente y servidor)
    Public = 10,
    /// Emote (cliente y servidor)
    Emote = 11,
    /// Mensaje personal / PM (cliente y servidor)
    PersonalMessage = 13,
    /// Fast ping keep-alive (cliente y servidor)
    FastPing = 14,
    /// User joined
    ServerJoin = 20,
    /// PM (privado)
    Pmt = 25,
    /// El receptor está ignorando al emisor
    ServerIsIgnoringYou = 26,
    /// Usuario destino del PM está offline
    ServerOfflineUser = 27,
    /// User parted
    ServerPart = 22,
    /// Lista de usuarios del canal
    ServerChannelUserList = 30,
    /// Topic de la sala
    ServerTopic = 31,
    /// Primer topic (al entrar)
    ServerTopicFirst = 32,
    /// Fin de la lista de usuarios
    ServerChannelUserListEnd = 35,
    /// HTML custom
    ServerHtml = 43,
    /// No such user / room
    ServerNosuch = 44,
    /// Lista de ignorados del cliente
    ClientIgnorelist = 45,
    /// Add file share
    ClientAddshare = 50,
    /// Remove file share
    ClientRemshare = 51,
    /// Browse request
    ClientBrowse = 52,
    /// End of browse results
    ServerEndOfBrowse = 53,
    /// Browse error
    ServerBrowseError = 54,
    /// Browse item
    ServerBrowseItem = 55,
    /// Start of browse results
    ServerStartOfBrowse = 56,
    /// Search
    ClientSearch = 60,
    /// Search hit
    ServerSearchHit = 61,
    /// End of search
    ServerEndOfSearch = 62,
    /// Dummy keep-alive
    ClientDummy = 64,
    /// Cliente envía lista de supernodes / Servidor responde con supernodes
    /// (mismo opcode, diferentes direcciones)
    Supernodes = 70,
    /// Direct chat push
    ClientDirchatPush = 72,
    /// URL tag para un usuario
    ServerUrl = 73,
    /// Comando slash del cliente
    ClientCommand = 74,
    /// Cambio de op / nivel
    ServerOpChange = 75,
    /// Datos comprimidos (zlib)
    ClientCompressed = 80,
    /// Auth login
    ClientAuthLogin = 82,
    /// Auth register
    ClientAuthRegister = 83,
    /// Features soportadas
    ServerMyFeatures = 92,

    /// Custom data (extensión sb0t)
    CustomData = 200,
    /// Custom data all
    CustomDataAll = 201,
    /// Add custom tags
    CustomAddTags = 202,
    /// Remove custom tags
    CustomRemTags = 203,
    /// Custom font (cliente y servidor)
    CustomFont = 204,
    /// Voice chat supported
    VcSupported = 205,
    /// Voice chat first packet
    VcFirst = 206,
    /// Voice chat from
    VcFirstFrom = 207,
    /// Voice chat chunk
    VcChunk = 208,
    /// Voice chat chunk from
    VcChunkFrom = 209,
    /// Voice chat ignore
    VcIgnore = 210,
    /// Voice chat no PM
    VcNoPvt = 211,
    /// Voice chat user supported
    VcUserSupported = 212,
    /// Protocolo de features avanzadas (sub-protocolo)
    AdvancedFeatures = 250,
    /// Room scribble
    ServerRoomScribble = 225,
    /// Crypto key exchange
    ServerCryptoKey = 230,
    /// Favicon
    ServerFavicon = 231,

    /// Scribble to room (first chunk)
    ClientScribbleRoomFirst = 240,
    /// Scribble to room (chunk)
    ClientScribbleRoomChunk = 241,
    /// Block custom names
    ClientBlockCustomNames = 242,

    /// Link protocol (Hub/Leaf entre servidores sb0t)
    LinkProto = 251,
}

impl TcpMsg {
    /// Convierte un byte al mensaje TCP correspondiente, si es válido.
    pub fn from_u8(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::ServerError,
            1 => Self::ClientRelogin,
            2 => Self::ClientLogin,
            3 => Self::ServerLoginAck,
            4 => Self::ClientUpdateStatus,
            5 => Self::ServerUpdateUserStatus,
            6 => Self::ServerRedirect,
            7 => Self::ClientAutologin,
            8 => Self::ServerEcho,
            9 => Self::Avatar,
            10 => Self::Public,
            11 => Self::Emote,
            13 => Self::PersonalMessage,
            14 => Self::FastPing,
            20 => Self::ServerJoin,
            22 => Self::ServerPart,
            25 => Self::Pmt,
            26 => Self::ServerIsIgnoringYou,
            27 => Self::ServerOfflineUser,
            30 => Self::ServerChannelUserList,
            31 => Self::ServerTopic,
            32 => Self::ServerTopicFirst,
            35 => Self::ServerChannelUserListEnd,
            43 => Self::ServerHtml,
            44 => Self::ServerNosuch,
            45 => Self::ClientIgnorelist,
            50 => Self::ClientAddshare,
            51 => Self::ClientRemshare,
            52 => Self::ClientBrowse,
            53 => Self::ServerEndOfBrowse,
            54 => Self::ServerBrowseError,
            55 => Self::ServerBrowseItem,
            56 => Self::ServerStartOfBrowse,
            60 => Self::ClientSearch,
            61 => Self::ServerSearchHit,
            62 => Self::ServerEndOfSearch,
            64 => Self::ClientDummy,
            70 => Self::Supernodes,
            72 => Self::ClientDirchatPush,
            73 => Self::ServerUrl,
            74 => Self::ClientCommand,
            75 => Self::ServerOpChange,
            80 => Self::ClientCompressed,
            82 => Self::ClientAuthLogin,
            83 => Self::ClientAuthRegister,
            92 => Self::ServerMyFeatures,
            200 => Self::CustomData,
            201 => Self::CustomDataAll,
            202 => Self::CustomAddTags,
            203 => Self::CustomRemTags,
            204 => Self::CustomFont,
            205 => Self::VcSupported,
            206 => Self::VcFirst,
            207 => Self::VcFirstFrom,
            208 => Self::VcChunk,
            209 => Self::VcChunkFrom,
            210 => Self::VcIgnore,
            211 => Self::VcNoPvt,
            212 => Self::VcUserSupported,
            225 => Self::ServerRoomScribble,
            230 => Self::ServerCryptoKey,
            231 => Self::ServerFavicon,
            240 => Self::ClientScribbleRoomFirst,
            241 => Self::ClientScribbleRoomChunk,
            242 => Self::ClientBlockCustomNames,
            250 => Self::AdvancedFeatures,
            251 => Self::LinkProto,
            _ => return None,
        })
    }
}

impl TryFrom<u8> for TcpMsg {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_u8(value).ok_or(value)
    }
}

/// Mensajes del protocolo UDP (ver `UdpMsg.cs`).
///
/// Usado para el room search (publicación de la sala en la red Ares)
/// y para el firewall check.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UdpMsg {
    /// Servidor envía su info al supernode
    ServerListSendInfo = 2,
    /// ACK del supernode
    ServerListAckInfo = 3,
    /// Añadir IPs
    ServerListAddIps = 11,
    /// ACK IPs
    ServerListAckIps = 12,
    /// Enviar nodos
    ServerListSendNodes = 21,
    /// ACK nodos
    ServerListAckNodes = 22,
    /// Cliente quiere chequear firewall
    ServerListWantCheckFirewall = 31,
    /// Cliente listo para check
    ServerListReadyToCheckFirewall = 32,
    /// Proceder con check
    ServerListProceedCheckFirewall = 33,
    /// Servidor ocupado
    ServerListCheckFirewallBusy = 34,
}

impl UdpMsg {
    /// Convierte un byte al mensaje UDP correspondiente, si es válido.
    pub fn from_u8(byte: u8) -> Option<Self> {
        Some(match byte {
            2 => Self::ServerListSendInfo,
            3 => Self::ServerListAckInfo,
            11 => Self::ServerListAddIps,
            12 => Self::ServerListAckIps,
            21 => Self::ServerListSendNodes,
            22 => Self::ServerListAckNodes,
            31 => Self::ServerListWantCheckFirewall,
            32 => Self::ServerListReadyToCheckFirewall,
            33 => Self::ServerListProceedCheckFirewall,
            34 => Self::ServerListCheckFirewallBusy,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_msg_roundtrip() {
        for byte in 0..=255u8 {
            if let Some(msg) = TcpMsg::from_u8(byte) {
                assert_eq!(msg as u8, byte);
            }
        }
    }

    #[test]
    fn udp_msg_roundtrip() {
        for byte in 0..=255u8 {
            if let Some(msg) = UdpMsg::from_u8(byte) {
                assert_eq!(msg as u8, byte);
            }
        }
    }

    #[test]
    fn tcp_msg_tryfrom() {
        assert_eq!(TcpMsg::try_from(2u8).unwrap(), TcpMsg::ClientLogin);
        assert!(TcpMsg::try_from(255u8).is_err());
    }
}

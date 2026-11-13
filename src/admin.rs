//! Canal admin: protocolo JSON-lines sobre TCP localhost entre a TUI e o daemon.
//!
//! A TUI conecta na `admin_port`, envia um `AdminRequest` por linha e recebe
//! um `AdminResponse` por linha.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum AdminRequest {
    /// Lista de terminais conectados
    ListTerminals,
    /// Últimas linhas de log (ring buffer do daemon)
    LogTail { n: usize },
    /// Config atual
    GetConfig,
    /// Aplica/salva config
    SetConfig { config: crate::config::Config },
    /// Envia mensagem instantânea (`#mesg`) a um terminal ou a todos (id = None)
    SendText {
        id: Option<String>,
        line1: String,
        line2: String,
        secs: u8,
    },
    /// Reinicia terminal(is) (`#restartsoft`)
    Restart { id: Option<String> },
    /// Consulta manual de preço (teste do pipeline sem terminal)
    LookupPrice { barcode: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub id: String,
    pub model: String,
    pub firmware: String,
    pub addr: String,
    pub connected_since: String,
    pub queries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ok", rename_all = "snake_case")]
pub enum AdminResponse {
    Success { data: AdminData },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdminData {
    Terminals {
        terminals: Vec<TerminalInfo>,
    },
    Log {
        lines: Vec<String>,
    },
    Config {
        config: crate::config::Config,
    },
    Done {
        detail: String,
    },
    Price {
        barcode: String,
        found: bool,
        name: Option<String>,
        price: Option<String>,
    },
}

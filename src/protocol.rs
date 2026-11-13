//! Protocolo TCP Gertec Busca-Preço (TC501/TC502/TC505).
//!
//! Especificação completa em `docs/PROTOCOL.md` (engenharia reversa dos
//! emuladores Java oficiais e headers da libTCgertec).
//!
//! Resumo do wire format:
//! - ASCII puro, delimitador `#` (0x23), sem CR/LF
//! - Campos de tamanho variável com prefixo de 1 dígito ASCII (len + '0')
//! - Handshake: servidor envia `#ok`; terminal responde `#<MODELO>|<FW>`
//! - Barras lidas: `#<digitos>`; resposta de preço `#<desc>|<preco>` ou `#nfound`

use std::io::{Read, Write};
use std::net::TcpStream;

/// ACKs que o terminal envia e que o servidor deve reconhecer/ignorar.
pub const KNOWN_ACKS: &[&str] = &[
    "live",
    "alwayslive_ok",
    "checklive_ok",
    "rextconf_ok",
    "rparamconfig_ok",
    "restartsoft_ok",
    "img_ok",
    "img_error",
];

/// Um comando recebido do terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalMsg {
    /// Identificação pós-handshake: `#TC502|2.10`
    Identify { model: String, firmware: String },
    /// Código de barras lido: `#7891234567890`
    Barcode(String),
    /// ACK conhecido (live, img_ok, ...)
    Ack(String),
    /// Qualquer outra coisa
    Unknown(String),
}

/// Divide um buffer coalescido em comandos individuais.
/// Comandos podem chegar concatenados no mesmo segmento TCP.
pub fn split_commands(buf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(buf).to_string();
    text.split('#')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Classifica um comando vindo do terminal (sem o `#` inicial).
pub fn classify(cmd: &str) -> TerminalMsg {
    if let Some((model, fw)) = cmd.split_once('|') {
        let model_u = model.to_uppercase();
        if model_u.starts_with("TC") && !model_u.contains(' ') {
            return TerminalMsg::Identify {
                model: model_u,
                firmware: fw.to_string(),
            };
        }
    }
    if cmd.chars().all(|c| c.is_ascii_digit()) && !cmd.is_empty() {
        return TerminalMsg::Barcode(cmd.to_string());
    }
    let lower = cmd.to_lowercase();
    if KNOWN_ACKS.contains(&lower.as_str()) {
        return TerminalMsg::Ack(lower);
    }
    TerminalMsg::Unknown(cmd.to_string())
}

/// Codifica um campo com prefixo de tamanho ASCII (1 dígito, máx. 9 bytes).
/// Retorna `None` se o campo não couber no formato (fidelidade ao original).
pub fn field(s: &str) -> Option<String> {
    let len = s.len();
    if len > 9 {
        return None;
    }
    Some(format!("{}{}", (len as u8 + b'0') as char, s))
}

// ---------- Mensagens servidor -> terminal (wire) ----------

pub fn hello() -> Vec<u8> {
    b"#ok".to_vec()
}

pub fn live_query() -> Vec<u8> {
    b"#live?".to_vec()
}

/// `#mesg` + L1 + L2 + tempo (2 dígitos ASCII, dezena e unidade).
pub fn mesg(line1: &str, line2: &str, secs: u8) -> Option<Vec<u8>> {
    let l1 = field(line1)?;
    let l2 = field(line2)?;
    let t = format!("{:02}", secs.min(99));
    Some(format!("#mesg{}{}{}", l1, l2, t).into_bytes())
}

/// Resposta de preço: `#<desc>|<preco>` — um único write, sem `\n`.
/// Preço truncado no primeiro `#` pelo terminal (coalescência segura).
pub fn price(desc: &str, price: &str) -> Vec<u8> {
    let d: String = desc
        .chars()
        .filter(|&c| c != '#' && c != '|')
        .take(20)
        .collect();
    let p: String = price
        .chars()
        .filter(|&c| c != '#' && c != '|')
        .take(20)
        .collect();
    format!("#{}|{}", d, p).into_bytes()
}

pub fn not_found() -> Vec<u8> {
    b"#nfound".to_vec()
}

/// `#restartsoft` + assinatura mágica `A5 CC 5A 33` nos bytes 12–15
/// (o firmware valida os 4 bytes mágicos logo após o comando).
pub fn restart() -> Vec<u8> {
    let mut v = b"#restartsoft".to_vec();
    v.extend_from_slice(&[0xA5, 0xCC, 0x5A, 0x33]);
    v
}

/// Envia um frame para o terminal em um único write.
pub fn send(stream: &mut TcpStream, frame: &[u8]) -> std::io::Result<()> {
    stream.write_all(frame)?;
    stream.flush()
}

/// Lê os bytes disponíveis (não bloqueante após set_read_timeout).
pub fn read_avail(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<usize> {
    stream.read(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_coalesced() {
        let cmds = split_commands(b"#live#789123#img_ok");
        assert_eq!(cmds, vec!["live", "789123", "img_ok"]);
    }

    #[test]
    fn classify_identify() {
        match classify("TC502|2.10") {
            TerminalMsg::Identify { model, firmware } => {
                assert_eq!(model, "TC502");
                assert_eq!(firmware, "2.10");
            }
            other => panic!("esperado Identify, veio {:?}", other),
        }
        match classify("tc505|1.20") {
            TerminalMsg::Identify { model, .. } => assert_eq!(model, "TC505"),
            other => panic!("esperado Identify, veio {:?}", other),
        }
    }

    #[test]
    fn classify_barcode() {
        assert_eq!(
            classify("7891234567890"),
            TerminalMsg::Barcode("7891234567890".into())
        );
    }

    #[test]
    fn classify_ack() {
        assert_eq!(classify("live"), TerminalMsg::Ack("live".into()));
        assert_eq!(
            classify("restartsoft_ok"),
            TerminalMsg::Ack("restartsoft_ok".into())
        );
    }

    #[test]
    fn mesg_wire() {
        // "#mesg" + campo3("OLA") + campo3("REC") + tempo "05"
        let f = mesg("OLA", "REC", 5).unwrap();
        assert_eq!(f, b"#mesg3OLA3REC05".to_vec());
    }

    #[test]
    fn price_wire() {
        assert_eq!(price("ARROZ 5KG", "3,99"), b"#ARROZ 5KG|3,99".to_vec());
    }

    #[test]
    fn restart_wire() {
        let f = restart();
        assert_eq!(&f[..12], b"#restartsoft");
        assert_eq!(&f[12..16], &[0xA5, 0xCC, 0x5A, 0x33]);
    }

    #[test]
    fn field_len_prefix() {
        assert_eq!(field("REC").unwrap(), "3REC");
        assert!(field("0123456789").is_none());
    }
}

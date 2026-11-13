//! Daemon principal: servidor TCP do protocolo (porta 6500) + canal admin (JSON-lines).

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::admin::{AdminData, AdminRequest, AdminResponse, TerminalInfo};
use crate::config::Config;
use crate::db::PriceDb;
use crate::protocol as proto;

const LOG_CAPACITY: usize = 1000;

pub struct TerminalHandle {
    pub info: TerminalInfo,
    pub writer: Mutex<TcpStream>,
    pub queries: std::sync::atomic::AtomicU64,
}

pub struct State {
    pub config: RwLock<Config>,
    /// Conexão com o banco, criada sob demanda (lazy) para não travar o listen.
    db: Mutex<Option<Arc<PriceDb>>>,
    /// Assinatura da última config de banco usada (url + tabela + colunas).
    db_sig: RwLock<String>,
    /// Último erro de conexão/config do banco (para o canal admin reportar).
    db_last_err: RwLock<Option<String>>,
    pub terminals: Mutex<HashMap<String, Arc<TerminalHandle>>>,
    pub log: Mutex<VecDeque<String>>,
    pub config_path: std::path::PathBuf,
}

impl State {
    pub fn new(config: Config, config_path: std::path::PathBuf) -> Arc<Self> {
        if config.database.url.is_empty() {
            eprintln!("[aviso] database.url vazio: consultas retornarao #nfound");
        }
        Arc::new(Self {
            config: RwLock::new(config),
            db: Mutex::new(None),
            db_sig: RwLock::new(String::new()),
            db_last_err: RwLock::new(None),
            terminals: Mutex::new(HashMap::new()),
            log: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            config_path,
        })
    }

    /// Pool do banco (conecta na primeira consulta; reconecta se a config mudou).
    pub fn db(&self) -> Option<Arc<PriceDb>> {
        let dbcfg = self.config.read().unwrap().database.clone();
        if dbcfg.url.is_empty() {
            return None;
        }
        let sig = format!(
            "{}|{}|{}|{}|{}",
            dbcfg.url, dbcfg.table, dbcfg.barcode_column, dbcfg.name_column, dbcfg.price_column
        );
        let mut slot = self.db.lock().unwrap();
        let current_sig = self.db_sig.read().unwrap().clone();
        if current_sig != sig {
            *slot = None;
            *self.db_sig.write().unwrap() = sig;
        }
        if slot.is_none() {
            match PriceDb::connect(&dbcfg) {
                Ok(db) => {
                    self.push_log(format!(
                        "[db] conexao estabelecida ({}.{})",
                        dbcfg.table, dbcfg.barcode_column
                    ));
                    *self.db_last_err.write().unwrap() = None;
                    *slot = Some(Arc::new(db));
                }
                Err(e) => {
                    self.push_log(format!("[db] falha ao conectar: {e}"));
                    *self.db_last_err.write().unwrap() = Some(e);
                    return None;
                }
            }
        }
        slot.clone()
    }

    /// Motivo da última falha do banco, se houver.
    pub fn db_error(&self) -> Option<String> {
        self.db_last_err.read().unwrap().clone()
    }

    pub fn push_log(&self, line: String) {
        if let Ok(mut l) = self.log.lock() {
            if l.len() == LOG_CAPACITY {
                l.pop_front();
            }
            l.push_back(line.clone());
        }
        println!("{line}");
    }
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Junta o prefixo de moeda ao preço (ex.: "R$" + "3,99" -> "R$ 3,99").
/// Prefixo vazio deixa o preço como está.
fn apply_currency(prefix: &str, price: &str) -> String {
    if prefix.trim().is_empty() {
        price.to_string()
    } else {
        format!("{} {}", prefix.trim(), price)
    }
}

// ----------------------------------------------------------------_TERMINAL--

pub fn run(state: Arc<State>) -> std::io::Result<()> {
    let port = state.config.read().unwrap().listen_port;
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    state.push_log(format!("[{}] daemon escutando na porta {port}", now()));

    // Canal admin em thread separada
    {
        let st = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = admin_server(st) {
                eprintln!("[erro] canal admin: {e}");
            }
        });
    }

    // Keepalive `#live?`
    {
        let st = state.clone();
        std::thread::spawn(move || keepalive_loop(st));
    }

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let st = state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_terminal(st, s) {
                        eprintln!("[conn] {e}");
                    }
                });
            }
            Err(e) => eprintln!("[erro] accept: {e}"),
        }
    }
    Ok(())
}

fn handle_terminal(state: Arc<State>, stream: TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(300))).ok();
    let peer = stream.peer_addr()?.to_string();
    let mut reader = stream.try_clone()?;
    let mut writer = stream;

    // Handshake: servidor envia #ok primeiro (spec §2)
    proto::send(&mut writer, &proto::hello())?;

    let mut handle = Arc::new(TerminalHandle {
        info: TerminalInfo {
            id: peer.clone(),
            model: "DESCONHECIDO".into(),
            firmware: String::new(),
            addr: peer.clone(),
            connected_since: now(),
            queries: 0,
        },
        writer: Mutex::new(writer.try_clone()?),
        queries: std::sync::atomic::AtomicU64::new(0),
    });

    let mut buf = [0u8; 7000]; // buffer do tamanho do legado
    let mut pending: Vec<u8> = Vec::new();
    let mut registered = false;

    loop {
        let n = match proto::read_avail(&mut reader, &mut buf) {
            Ok(0) => break, // conexão fechada
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(_) => break,
        };
        pending.extend_from_slice(&buf[..n]);

        // Processa todos os comandos coalescidos; se sobrar fragmento sem
        // significado completo, mantém em `pending` para a próxima leitura.
        let cmds = proto::split_commands(&pending);
        pending.clear();
        for cmd in cmds {
            match proto::classify(&cmd) {
                proto::TerminalMsg::Identify { model, firmware } => {
                    handle = Arc::new(TerminalHandle {
                        info: TerminalInfo {
                            id: format!("{model}@{peer}"),
                            model: model.clone(),
                            firmware,
                            addr: peer.clone(),
                            connected_since: now(),
                            queries: 0,
                        },
                        writer: Mutex::new(writer.try_clone()?),
                        queries: std::sync::atomic::AtomicU64::new(0),
                    });
                    state
                        .terminals
                        .lock()
                        .unwrap()
                        .insert(handle.info.id.clone(), handle.clone());
                    registered = true;
                    state.push_log(format!(
                        "[{}] terminal conectado: {}",
                        now(),
                        handle.info.id
                    ));
                }
                proto::TerminalMsg::Barcode(code) => {
                    state.push_log(format!("[{}] {} leu codigo {code}", now(), handle.info.id));
                    handle
                        .queries
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let prefix = state.config.read().unwrap().currency_prefix.clone();
                    let reply = match state.db() {
                        Some(db) => match db.lookup(&code) {
                            Ok(Some(p)) => {
                                let price = apply_currency(&prefix, &p.price);
                                state.push_log(format!(
                                    "[{}] preco para {code}: {} {}",
                                    now(),
                                    p.name,
                                    price
                                ));
                                proto::price(&p.name, &price)
                            }
                            Ok(None) => proto::not_found(),
                            Err(e) => {
                                state.push_log(format!("[erro] consulta {code}: {e}"));
                                proto::not_found()
                            }
                        },
                        None => {
                            state.push_log("[aviso] sem banco configurado".into());
                            proto::not_found()
                        }
                    };
                    if let Ok(mut w) = handle.writer.lock() {
                        proto::send(&mut w, &reply)?;
                    }
                }
                proto::TerminalMsg::Ack(ack) => {
                    // keepalive/acks: nada a fazer (spec §5.3)
                    let _ = ack;
                }
                proto::TerminalMsg::Unknown(rest) => {
                    state.push_log(format!("[{}] comando nao reconhecido: {rest}", now()));
                }
            }
        }
    }

    if registered {
        state.terminals.lock().unwrap().remove(&handle.info.id);
    }
    state.push_log(format!(
        "[{}] terminal desconectado: {}",
        now(),
        handle.info.id
    ));
    Ok(())
}

fn keepalive_loop(state: Arc<State>) {
    loop {
        let secs = state.config.read().unwrap().keepalive_secs.max(10);
        std::thread::sleep(Duration::from_secs(secs));
        let terminals: Vec<Arc<TerminalHandle>> =
            state.terminals.lock().unwrap().values().cloned().collect();
        for t in terminals {
            if let Ok(mut w) = t.writer.lock() {
                if proto::send(&mut w, &proto::live_query()).is_err() {
                    state.push_log(format!("[{}] keepalive falhou: {}", now(), t.info.id));
                }
            }
        }
    }
}

// -------------------------------------------------------------------ADMIN--

fn admin_server(state: Arc<State>) -> std::io::Result<()> {
    let port = state.config.read().unwrap().admin_port;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    state.push_log(format!(
        "[{}] canal admin escutando em 127.0.0.1:{port}",
        now()
    ));
    for stream in listener.incoming() {
        let st = state.clone();
        if let Ok(s) = stream {
            std::thread::spawn(move || {
                if let Err(e) = handle_admin(st, s) {
                    eprintln!("[admin] {e}");
                }
            });
        }
    }
    Ok(())
}

fn handle_admin(state: Arc<State>, stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let req: AdminRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                write_json(
                    &mut writer,
                    &AdminResponse::Error {
                        message: format!("requisição inválida: {e}"),
                    },
                )?;
                continue;
            }
        };
        let resp = dispatch(&state, req);
        write_json(&mut writer, &resp)?;
    }
    Ok(())
}

fn write_json(w: &mut TcpStream, resp: &AdminResponse) -> std::io::Result<()> {
    let mut j = serde_json::to_string(resp).unwrap();
    j.push('\n');
    w.write_all(j.as_bytes())?;
    w.flush()
}

fn dispatch(state: &Arc<State>, req: AdminRequest) -> AdminResponse {
    match req {
        AdminRequest::ListTerminals => {
            let terminals: Vec<TerminalInfo> = state
                .terminals
                .lock()
                .unwrap()
                .values()
                .map(|t| {
                    let mut info = t.info.clone();
                    info.queries = t.queries.load(std::sync::atomic::Ordering::Relaxed);
                    info
                })
                .collect();
            AdminResponse::Success {
                data: Box::new(AdminData::Terminals { terminals }),
            }
        }
        AdminRequest::LogTail { n } => {
            let lines = state
                .log
                .lock()
                .unwrap()
                .iter()
                .rev()
                .take(n.min(LOG_CAPACITY))
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            AdminResponse::Success {
                data: Box::new(AdminData::Log { lines }),
            }
        }
        AdminRequest::GetConfig => AdminResponse::Success {
            data: Box::new(AdminData::Config {
                config: state.config.read().unwrap().clone(),
            }),
        },
        AdminRequest::SetConfig { config } => {
            if let Err(e) = config.save(&state.config_path) {
                return AdminResponse::Error {
                    message: format!("ao salvar config: {e}"),
                };
            }
            *state.config.write().unwrap() = config;
            AdminResponse::Success {
                data: Box::new(AdminData::Done {
                    detail: "config salva (reinicie para reconectar o banco)".into(),
                }),
            }
        }
        AdminRequest::SendText {
            id,
            line1,
            line2,
            secs,
        } => match proto::mesg(&line1, &line2, secs) {
            Some(frame) => send_to(state, id.as_deref(), &frame, "send_text"),
            None => AdminResponse::Error {
                message: "linhas devem ter até 9 caracteres".into(),
            },
        },
        AdminRequest::Restart { id } => {
            let frame = proto::restart();
            send_to(state, id.as_deref(), &frame, "restart")
        }
        AdminRequest::LookupPrice { barcode } => {
            let prefix = state.config.read().unwrap().currency_prefix.clone();
            match state.db() {
                None => AdminResponse::Error {
                    message: state
                        .db_error()
                        .unwrap_or_else(|| "sem banco configurado".into()),
                },
                Some(db) => match db.lookup(&barcode) {
                    Ok(Some(p)) => AdminResponse::Success {
                        data: Box::new(AdminData::Price {
                            barcode,
                            found: true,
                            name: Some(p.name),
                            price: Some(apply_currency(&prefix, &p.price)),
                        }),
                    },
                    Ok(None) => AdminResponse::Success {
                        data: Box::new(AdminData::Price {
                            barcode,
                            found: false,
                            name: None,
                            price: None,
                        }),
                    },
                    Err(e) => AdminResponse::Error { message: e },
                },
            }
        }
    }
}

fn send_to(state: &Arc<State>, id: Option<&str>, frame: &[u8], what: &str) -> AdminResponse {
    let terminals: Vec<Arc<TerminalHandle>> = state
        .terminals
        .lock()
        .unwrap()
        .values()
        .filter(|t| id.is_none_or(|i| t.info.id == i))
        .cloned()
        .collect();
    if terminals.is_empty() {
        return AdminResponse::Error {
            message: "nenhum terminal corresponde".into(),
        };
    }
    let mut sent = 0;
    for t in terminals {
        if let Ok(mut w) = t.writer.lock() {
            if proto::send(&mut w, frame).is_ok() {
                sent += 1;
            }
        }
    }
    state.push_log(format!("[{}] admin {what} -> {sent} terminal(is)", now()));
    AdminResponse::Success {
        data: Box::new(AdminData::Done {
            detail: format!("{sent} terminal(is)"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_currency;

    #[test]
    fn currency_prefix_aplicado() {
        assert_eq!(apply_currency("R$", "3,99"), "R$ 3,99");
        assert_eq!(apply_currency("R$", "12,50"), "R$ 12,50");
    }

    #[test]
    fn currency_prefix_vazio() {
        assert_eq!(apply_currency("", "3,99"), "3,99");
        assert_eq!(apply_currency("  ", "3,99"), "3,99");
    }
}

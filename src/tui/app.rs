//! Estado da TUI.

use crate::admin::TerminalInfo;
use crate::config::Config;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Config,
    Terminais,
    Preco,
    Log,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::Config => Tab::Terminais,
            Tab::Terminais => Tab::Preco,
            Tab::Preco => Tab::Log,
            Tab::Log => Tab::Config,
        }
    }
}

pub struct MsgPrompt {
    pub target: Option<String>,
    pub fields: [String; 3],
    pub idx: usize,
}

impl MsgPrompt {
    pub fn new(target: Option<String>) -> Self {
        Self {
            target,
            fields: [String::new(), String::new(), "5".into()],
            idx: 0,
        }
    }
}

pub struct App {
    pub tab: Tab,
    pub config_path: PathBuf,
    pub edit: Config,
    pub selected: usize,
    pub cursor: usize,
    pub daemon_on: bool,
    pub status: String,
    pub terminals: Vec<TerminalInfo>,
    pub term_selected: usize,
    pub msg: Option<MsgPrompt>,
    pub price_input: String,
    pub price_history: Vec<String>,
    pub log_lines: Vec<String>,
    pub log_scroll: u16,
    pub log_follow: bool,
    pub log_viewport: u16,
    pub last_cfg_check: Instant,
    pub last_term_refresh: Instant,
    pub last_log_refresh: Instant,
}

pub const FIELDS: [(&str, bool); 12] = [
    ("database.url", false),
    ("database.table", false),
    ("database.barcode_column", false),
    ("database.name_column", false),
    ("database.price_column", false),
    ("listen_port", true),
    ("admin_port", true),
    ("display_time_secs", true),
    ("keepalive_secs", true),
    ("currency_prefix", false),
    ("idle_line1", false),
    ("idle_line2", false),
];

impl App {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let cursor = field_value(&config, 0).len();
        Self {
            tab: Tab::Config,
            config_path,
            edit: config,
            selected: 0,
            cursor,
            daemon_on: false,
            status: String::new(),
            terminals: Vec::new(),
            term_selected: 0,
            msg: None,
            price_input: String::new(),
            price_history: Vec::new(),
            log_lines: Vec::new(),
            log_scroll: 0,
            log_follow: true,
            log_viewport: 0,
            last_cfg_check: Instant::now(),
            last_term_refresh: Instant::now(),
            last_log_refresh: Instant::now(),
        }
    }

    pub fn daemon_port(&self) -> u16 {
        self.edit.admin_port
    }
}

pub fn field_value(cfg: &Config, i: usize) -> String {
    match i {
        0 => cfg.database.url.clone(),
        1 => cfg.database.table.clone(),
        2 => cfg.database.barcode_column.clone(),
        3 => cfg.database.name_column.clone(),
        4 => cfg.database.price_column.clone(),
        5 => cfg.listen_port.to_string(),
        6 => cfg.admin_port.to_string(),
        7 => cfg.display_time_secs.to_string(),
        8 => cfg.keepalive_secs.to_string(),
        9 => cfg.currency_prefix.clone(),
        10 => cfg.idle_line1.clone(),
        11 => cfg.idle_line2.clone(),
        _ => String::new(),
    }
}

pub fn set_field_value(cfg: &mut Config, i: usize, v: &str) {
    match i {
        0 => cfg.database.url = v.to_string(),
        1 => cfg.database.table = v.to_string(),
        2 => cfg.database.barcode_column = v.to_string(),
        3 => cfg.database.name_column = v.to_string(),
        4 => cfg.database.price_column = v.to_string(),
        5 => {
            if let Ok(n) = v.parse() {
                cfg.listen_port = n;
            }
        }
        6 => {
            if let Ok(n) = v.parse() {
                cfg.admin_port = n;
            }
        }
        7 => {
            if let Ok(n) = v.parse() {
                cfg.display_time_secs = n;
            }
        }
        8 => {
            if let Ok(n) = v.parse() {
                cfg.keepalive_secs = n;
            }
        }
        9 => cfg.currency_prefix = v.to_string(),
        10 => cfg.idle_line1 = v.to_string(),
        11 => cfg.idle_line2 = v.to_string(),
        _ => {}
    }
}

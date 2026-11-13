use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    /// mysql://... ou postgres://...
    #[serde(default)]
    pub url: String,
    /// Tabela de produtos
    #[serde(default = "Database::default_table")]
    pub table: String,
    #[serde(default = "Database::default_barcode_column")]
    pub barcode_column: String,
    #[serde(default = "Database::default_name_column")]
    pub name_column: String,
    #[serde(default = "Database::default_price_column")]
    pub price_column: String,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            url: String::new(),
            table: Database::default_table(),
            barcode_column: Database::default_barcode_column(),
            name_column: Database::default_name_column(),
            price_column: Database::default_price_column(),
        }
    }
}

impl Database {
    fn default_table() -> String {
        "pricetable".into()
    }
    fn default_barcode_column() -> String {
        "barcode".into()
    }
    fn default_name_column() -> String {
        "name".into()
    }
    fn default_price_column() -> String {
        "price".into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Porta TCP do protocolo dos terminais (default Gertec: 6500)
    pub listen_port: u16,
    /// Porta TCP admin (localhost, JSON-lines) usada pela TUI
    pub admin_port: u16,
    /// Configuração do banco de dados (tabela e colunas customizáveis)
    #[serde(default)]
    pub database: Database,
    /// Segundos que o display exibe o preço
    pub display_time_secs: u8,
    /// Linhas exibidas no display em idle (mensagem instantânea de boas-vindas)
    pub idle_line1: String,
    pub idle_line2: String,
    /// Intervalo do keepalive `#live?` em segundos (deve ser < 120)
    pub keepalive_secs: u64,
    /// Prefixo exibido antes do preço no display (ex.: "R$")
    #[serde(default = "Config::default_currency_prefix")]
    pub currency_prefix: String,
    /// Caminho do arquivo de log (além do admin/log da TUI)
    #[serde(default)]
    pub log_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_port: 6500,
            admin_port: 6501,
            database: Database::default(),
            display_time_secs: 5,
            idle_line1: "Busca Preco".into(),
            idle_line2: "Passe o produto".into(),
            keepalive_secs: 60,
            currency_prefix: Config::default_currency_prefix(),
            log_file: None,
        }
    }
}

impl Config {
    fn default_currency_prefix() -> String {
        "R$".into()
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // compat: database_url do nível antigo vale se [database].url estiver vazio
        #[derive(serde::Deserialize)]
        struct Legacy {
            #[serde(default)]
            database_url: String,
        }
        if let Ok(legacy) = toml::from_str::<Legacy>(&raw) {
            if cfg.database.url.is_empty() && !legacy.database_url.is_empty() {
                cfg.database.url = legacy.database_url;
            }
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let raw = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, raw)
    }
}

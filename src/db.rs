//! Consulta de preço — MySQL ou PostgreSQL via `sqlx` (Any).
//!
//! Tabela e colunas são customizáveis via `[database]` do config.toml.
//! Exemplo com schema padrão:
//! ```sql
//! CREATE TABLE pricetable (
//!   barcode VARCHAR(32) PRIMARY KEY,
//!   name    VARCHAR(100) NOT NULL,
//!   price   NUMERIC(10,2) NOT NULL
//! );
//! ```

use std::time::Duration;

use sqlx::any::AnyPoolOptions;
use sqlx::{any::install_default_drivers, AnyPool, Row};

use crate::config::Database;

pub struct Product {
    pub name: String,
    pub price: String,
}

/// Identificadores SQL aceitos: apenas [A-Za-z0-9_.] (permite "esquema.tabela").
/// Placeholders não servem para nomes de tabela/coluna, então validamos aqui
/// para evitar injeção SQL via configuração.
pub fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && s.chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && !s.ends_with('.')
}

/// Mapeamento tabela/colunas validado, pronto para montar a query.
pub struct DbMapping {
    table: String,
    barcode: String,
    name: String,
    price: String,
}

impl DbMapping {
    pub fn new(cfg: &Database) -> Result<Self, String> {
        for (label, v) in [
            ("table", &cfg.table),
            ("barcode_column", &cfg.barcode_column),
            ("name_column", &cfg.name_column),
            ("price_column", &cfg.price_column),
        ] {
            if !valid_ident(v) {
                return Err(format!(
                    "database.{label} = \"{v}\" é um identificador SQL inválido \
                     (use apenas letras, números, _ e .)"
                ));
            }
        }
        Ok(Self {
            table: cfg.table.clone(),
            barcode: cfg.barcode_column.clone(),
            name: cfg.name_column.clone(),
            price: cfg.price_column.clone(),
        })
    }

    /// Query parametrizada para o dialeto; aliases bp_name/bp_price evitam
    /// colisão quando a coluna customizada tem nome igual ao alias.
    fn sql(&self, is_postgres: bool) -> String {
        if is_postgres {
            format!(
                "SELECT {name} AS bp_name, {price}::text AS bp_price FROM {table} WHERE {barcode} = $1",
                name = self.name,
                price = self.price,
                table = self.table,
                barcode = self.barcode
            )
        } else {
            format!(
                "SELECT {name} AS bp_name, CAST({price} AS CHAR) AS bp_price FROM {table} WHERE {barcode} = ?",
                name = self.name,
                price = self.price,
                table = self.table,
                barcode = self.barcode
            )
        }
    }
}

pub struct PriceDb {
    pool: AnyPool,
    is_postgres: bool,
    mapping: DbMapping,
    tokio: tokio::runtime::Runtime,
}

impl PriceDb {
    pub fn connect(cfg: &Database) -> Result<Self, String> {
        install_default_drivers();
        let url = cfg.url.trim();
        let is_postgres = url.starts_with("postgres://") || url.starts_with("postgresql://");
        if !(is_postgres || url.starts_with("mysql://") || url.starts_with("mariadb://")) {
            return Err("database.url deve ser mysql://... ou postgres://...".into());
        }
        let mapping = DbMapping::new(cfg)?;
        let tokio = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let pool = tokio
            .block_on(
                AnyPoolOptions::new()
                    .max_connections(5)
                    .acquire_timeout(Duration::from_secs(10))
                    .connect(url),
            )
            .map_err(|e| format!("falha ao conectar no banco: {e}"))?;
        Ok(Self {
            pool,
            is_postgres,
            mapping,
            tokio,
        })
    }

    /// Busca exata por código de barras (equivalente ao SeekBarCode do legado).
    /// Preço vem como texto (CAST/::text) porque o driver Any não mapeia DECIMAL.
    pub fn lookup(&self, barcode: &str) -> Result<Option<Product>, String> {
        let sql = self.mapping.sql(self.is_postgres);
        let barcode = barcode.to_string();
        let row = self.tokio.block_on(async {
            sqlx::query(&sql)
                .bind(&barcode)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())
        })?;
        Ok(row.map(|r| Product {
            name: r.try_get::<String, _>("bp_name").unwrap_or_default(),
            price: r
                .try_get::<String, _>("bp_price")
                .ok()
                .and_then(|p| p.parse::<f64>().ok())
                .map(format_price)
                .unwrap_or_default(),
        }))
    }
}

/// Formata o preço no padrão brasileiro (vírgula decimal), como o terminal espera.
fn format_price(p: f64) -> String {
    format!("{:.2}", p).replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_cfg(table: &str, barcode: &str, name: &str, price: &str) -> Database {
        Database {
            url: String::new(),
            table: table.into(),
            barcode_column: barcode.into(),
            name_column: name.into(),
            price_column: price.into(),
        }
    }

    #[test]
    fn price_br_format() {
        assert_eq!(format_price(3.99), "3,99");
        assert_eq!(format_price(12.5), "12,50");
    }

    #[test]
    fn ident_validos() {
        for ok in ["produtos", "preco_venda", "esquema.produtos", "col_2", "_x"] {
            assert!(valid_ident(ok), "deveria aceitar {ok}");
        }
    }

    #[test]
    fn ident_invalidos() {
        for bad in [
            "",
            "tabela; DROP TABLE x",
            "col\"x",
            "1comeca_numero",
            "a-b",
            "tab.",
            "para-choque",
            "coluna--comentario",
        ] {
            assert!(!valid_ident(bad), "deveria rejeitar {bad:?}");
        }
    }

    #[test]
    fn mapping_rejeita_injecao() {
        assert!(DbMapping::new(&db_cfg("produtos;--", "codigo", "nome", "preco")).is_err());
        assert!(DbMapping::new(&db_cfg("produtos", "codigo", "nome", "preco")).is_ok());
    }

    #[test]
    fn sql_customizado() {
        let m = DbMapping::new(&db_cfg("produtos", "codigo", "descricao", "valor")).unwrap();
        assert_eq!(
            m.sql(true),
            "SELECT descricao AS bp_name, valor::text AS bp_price FROM produtos WHERE codigo = $1"
        );
        assert_eq!(
            m.sql(false),
            "SELECT descricao AS bp_name, CAST(valor AS CHAR) AS bp_price FROM produtos WHERE codigo = ?"
        );
    }

    #[test]
    fn sql_default() {
        let m = DbMapping::new(&Database::default()).unwrap();
        assert_eq!(
            m.sql(false),
            "SELECT name AS bp_name, CAST(price AS CHAR) AS bp_price FROM pricetable WHERE barcode = ?"
        );
    }
}

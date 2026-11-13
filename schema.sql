-- Schema do servidor de busca de preço (MySQL e PostgreSQL compatíveis)
-- Ver docs/SERVICE_ANALYSIS.md para a origem (tabelas_Mysql.c / tabelas_PostGre.c)

CREATE TABLE IF NOT EXISTS pricetable (
  barcode VARCHAR(32) PRIMARY KEY,
  name    VARCHAR(100) NOT NULL,
  price   NUMERIC(10,2) NOT NULL
);

-- Cadastro de terminais (opcional; hoje o daemon registra automaticamente)
CREATE TABLE IF NOT EXISTS terminals (
  ip   VARCHAR(45) PRIMARY KEY,
  name VARCHAR(50) NOT NULL,
  kind VARCHAR(10) NOT NULL DEFAULT 'TC501'
);

INSERT INTO pricetable (barcode, name, price) VALUES
  ('7891234567890', 'ARROZ 5KG', 3.99),
  ('7899876543210', 'FEIJAO 1KG', 7.49);

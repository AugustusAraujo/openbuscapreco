# buscapreco-rs

Port em Rust do servidor Gertec **Busca Preço** (ServUni Linux), reescrito a partir de
engenharia reversa dos softwares originais (2007–2012). Substitui o par
`TCgertec`/`server` + `libTCgertec.so` (proprietária e indisponível) por um binário
único, aberto, 64-bit.

- **Protocolo**: TCP porta **6500**, ASCII com framing `#` (handshake `#ok` → `#TC502|2.10`,
  barras `#<código>`, resposta `#<desc>|<preço>` / `#nfound`, keepalive `#live?`,
  `#mesg`, `#restartsoft` + mágico `A5 CC 5A 33`) — especificação completa em
  [`docs/PROTOCOL.md`](docs/PROTOCOL.md)
- **Terminais suportados (v1)**: perfil TC501/TC502 (busca preço clássico). O perfil TC505
  usa o mesmo wire format ([`docs/TC505_NOTES.md`](docs/TC505_NOTES.md))
- **BD**: MySQL ou PostgreSQL, com tabela/colunas customizáveis via `[database]` do config

## Comandos

```bash
buscapreco init          # cria config.toml de exemplo
buscapreco serve         # daemon (protocolo :6500 + canal admin :6501)
buscapreco tui           # TUI de configuração/administração
buscapreco healthcheck   # usado pelo Docker (verifica a porta do protocolo)
```

## TUI (ratatui)

| Tecla | Ação |
|---|---|
| `1`–`4` / `Tab` | Abas: Config, Terminais, Preço, Log |
| Config | setas navegam/editam campos; `Enter` salva e aplica no daemon |
| Terminais | `m` envia mensagem, `r` restart, `Enter` ação no selecionado |
| Preço | digitar código + `Enter` → consulta no BD |
| `q` / `Esc` | sair |

A TUI fala com o daemon pelo canal admin (JSON-lines em `127.0.0.1:admin_port`),
inclusive quando o daemon roda no Docker (porta admin publicada só em localhost).

## Docker

```bash
cp config.example.toml config.toml   # ajuste [database] (ou export DATABASE_URL)
docker compose up -d --build
```

`docker-compose.yml` publica `6500` (terminais) e `127.0.0.1:6501` (TUI), com
volume para `config.toml` e `log/`. Schema: `schema.sql`.

### Banco de dados

Tabela e colunas são customizáveis no `[database]` do `config.toml`
(`table`, `barcode_column`, `name_column`, `price_column` — apenas letras,
números, `_` e `.`). Exemplo com schema default:

```sql
CREATE TABLE pricetable (
  barcode VARCHAR(32) PRIMARY KEY,
  name    VARCHAR(100) NOT NULL,
  price   NUMERIC(10,2) NOT NULL
);
```

Preço é formatado no padrão brasileiro (`3.99` → `3,99` no display).

## Validação com o emulador oficial

O protocolo foi validado contra um simulador byte a byte derivado do
`Terminal.java` descompilado. Para validar com o emulador Java oficial (GUI):

1. No host com X: `java -jar Emu_Buscapreco.jar` (pasta do emulador precisa de
   `images/` e `config.txt` com `IPServer=<ip-do-servidor>`)
2. Menu *Terminal → Iniciar* — o display deve mostrar "Conectado"
3. Digitar um código existente no campo de barras e conferir o preço no display

## Estrutura

```
src/main.rs      CLI (clap): serve | tui | init | healthcheck
src/daemon.rs    servidor TCP do protocolo + canal admin + keepalive
src/protocol.rs  frames do protocolo (docs/PROTOCOL.md)
src/db.rs        consulta de preço (sqlx, MySQL/PostgreSQL)
src/tui/         TUI ratatui (Config, Terminais, Preço, Log)
docs/            engenharia reversa: PROTOCOL.md, SERVICE_ANALYSIS.md, TC505_NOTES.md
```

## Limitações da v1

Loop de publicidade/imagens casadas, comandos estendidos do TC505 (`#extconfig`,
`#updconfig`...) e protocolo binário do TC504 não implementados — ver
`docs/SERVICE_ANALYSIS.md` para o comportamento original.

## CI/CD

- **CI** (`.github/workflows/ci.yml`): fmt + clippy + testes + build em cada push/PR
- **Release** (`.github/workflows/release.yml`): ao dar push na `main`, a versão
  semântica é incrementada automaticamente a partir dos **conventional commits**
  (`feat:` → minor, `fix:` → patch, `BREAKING CHANGE:`/`!` → major), criando tag,
  GitHub Release com o binário linux-amd64 e a imagem Docker em
  `ghcr.io/<owner>/<repo>` (tags `vX.Y.Z`, `X.Y` e `latest`)

## Licença e origem

Código novo (MIT). Derivado de engenharia reversa de artefatos Gertec para
interoperabilidade com terminais já adquiridos.

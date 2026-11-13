# Análise do serviço Gertec Busca Preço (ServUni Linux, 2007)

Documento de referência para o port em Rust (`buscapreco-rs`), baseado nos fontes C
originais em `SCgertec/`: `TCgertec.c` (daemon), `server.c` (CLI de administração),
`tabelas*.c/.h` (backends TXT/MySQL/PostgreSQL/SQLite), `libTCgertec.h` (API da
biblioteca proprietária de comunicação com os terminais), `msq.h` (SysV message
queues), `images.c` e `tables/*.txt`.

---

## 1. Arquitetura e fluxo

### 1.1 Visão geral

```
+-----------+  UDP/TCP (lib libTCgertec.so)  +-----------+  SysV msq (key=10)  +-----------+
| Terminais | <----------------------------> |  TCgertec | <-----------------> |  server   |
| TC501/502 |    Busca Preço / TC505         |  (daemon) |   type 1 <-> 2      |  (CLI)    |
+-----------+                                +-----------+                     +-----------+
                                                  |  tables/pricetab.txt
                                                  |  tables/cardtable.txt
                                                  |  tables/names.txt
                                                  |  images/*.bmp, images/images.txt
                                                  |  (variantes: MySQL/PostgreSQL/SQLite)
                                                  +-> log/log_MM_AAAA.txt
```

- **`TCgertec`** (`TCgertec.c`): daemon principal. Cria a message queue SysV
  (`msgget(MSG_KEY=10, 0600|IPC_CREAT)`), abre o log `log/log_<mês>_<ano>.txt` e
  lança 4 threads pthreads:
  - `Start` → `StartServer()` (lib): aceita conexões e mantém a tabela de
    terminais conectados `TabConec` (até `MAX_TC=200`: `ID[]`, `IP[]`,
    `type[]` — 0 = protocolo TC501/502/505 (texto), 1 = TC504 (gráfico) —,
    `name[][20]`, `NumTerm`).
  - `Check` → eventos dos terminais tipo 0 (TC501 etc.).
  - `Check_504` → eventos dos terminais TC504 (barcode, cartão MCR, teclado).
  - `Menu_504` → navegação por botões (a/b/c/d) do TC504, via msq tipo 5,
    trocando imagens `images/img1..8.bmp` e parando/reiniciando o loop de
    publicidade.
  - Loop `main()`: `msgrcv(...,type=1,0)` — fica bloqueado esperando comandos
    enviados pelo `server` (STOP, SEND_TEXT, SEND_IMAGE, GET_CONFIG, S_LEC,
    S_TIME, S_KEY, RESTART, CLEAR, START/STOP_LOOP, SEND_FILE, RELOAD/DELETE_ADV,
    RELOAD/DELETE_PRE, SEND_CONFIG/UPD/PARAM/SETUP, TAB_CONEC) e despacha para a
    função correspondente da lib, sempre gravando no log.
- **`server`** (`server.c`): CLI interativo (`SCGertec-> `), conecta-se à mesma
  msq (`msgget(MSG_KEY,0)` — exige o daemon rodando), thread `Receive_MSG`
  recebe respostas tipo 2 (tabela de conectados, configs). Carrega idioma de
  `languages/padrao.txt`.
- **Comunicação interna** (`msq.h`): `MSG_DATA { long type; int CMD; int ID;
  char data[7000]; }`, `MSG_LENGHT = 7008`, permissões `0600`. Daemon consome
  `type=1`, responde com `type=2`; o menu TC504 usa `type=5`
  (`Menu_TC504 { long type; char comando; int identificador; }`).

### 1.2 Fluxo completo de uma consulta de preço

**Terminal texto (TC501/502 "Busca Preço", type=0)** — thread `Check`
(TCgertec.c:569):

1. O terminal conecta ao servidor (`StartServer`/lib registra em `TabConec`).
2. O terminal lê um código de barras e envia a string `#<código>`; a lib
   entrega via `CheckCMD(buffercmd)` que retorna o `ID` do terminal.
3. Se o buffer começa com `#`: ignoram-se os acks `#img_ok`, `#img_error`,
   `#restartsoft_ok`; senão chama-se
   `SeekBarCode("./tables/pricetab.txt", &buffercmd[1], &achou)`
   (ou a variante MySQL/PostgreSQL/SQLite conforme a build).
4. `SeekBarCode` varre o arquivo linha a linha comparando o 1º campo (código)
   com `strcmp`; em caso de igualdade faz parse dos campos separados por `|`
   e devolve o struct `Preco` (ver §3.1); `achou=1`.
5. **Achou**: `LOG_RECEIVE(código, ID, 0, log)` grava
   `IP;código;dd/mm/aaaa;hh:mm` e chama-se **`SendProductPrice(ID, produto.nome,
   produto.preco)`** (lib) — envia nome e preço ao terminal, que exibe por
   `Config.time` (segundos configurável) e volta às linhas idle `line1..line4`.
6. **Não achou**: **`SendProductNotFound(ID)`** — o terminal exibe mensagem
   padrão de "produto não encontrado".
7. Se a mensagem não começa com `#`, é handshake/identificação de terminal
   compatível com imagens (TC505): o daemon responde `SendImageList(ID)`
   (images.c:99), que envia `upd.bmp`, um "reset" da lista
   (`SendImageFromFile(ID,NULL,0,0,0,0xFF,0,0)`), todas as propagandas de
   `images/images.txt` (`arquivo|tempo|loop`, 128x64, 8bpp, índice incremental)
   e `upd.bmp` de novo — o terminal então roda o loop de publicidade sozinho
   (sem polling do servidor).

**Terminal gráfico TC504 (type=1)** — thread `Check_504` (TCgertec.c:417):

1. `CheckCMD_504(&ID,&CMD)` recebe o comando: `BARCODE=89`, `MCR=69`,
   `CHAR=53`.
2. `BARCODE`: `ReceiveBAR(ID,Bar)` (remove o `\n` final), `SeekBarCode(...)`;
   se achou: `LoadImage(ID,1)` (seleciona frame), `SendImage(ID,
   "images/<imagem1>")` (images.c:151 — faz parse do BMP, extrai paleta 256
   cores em RGB, inverte as linhas do raster 320-wide e envia via
   `SendImage_504`), e dois `SendText_504` com `ARG_DISPLAY_TEXT` (nome na
   posição `Xnome,Ynome` e preço em `Xpreco,Ypreco`, cor
   `ConvertCor(produto.cor)`, fundo `ConvertCor(produto.background)`, fonte
   `c:\fonts\LUCIDA16.BMP`). Se houver `imagem2`, cria a thread `Casadas` que
   após 5 s envia `imagem2` e, se houver `imagem3`, após mais 5 s envia
   `imagem3` (imagens "casadas" = slides adicionais do produto).
   Se não achou: `SendImage(ID,"images/notfound.bmp")`.
3. `MCR` (cartão magnético): `ReceiveMCR` lê track1+track2 concatenados,
   `SeekBonus("./tables/cardtable.txt", trilhas, &achou)`; se achou, envia
   `LoadImage(ID,2)`, imagem `bonus.imagem` e dois textos fixos (nome em
   Y=195 amarelo/preto, bônus em Y=215 branco/preto, fonte LUCIDA12);
   senão `SendImage(ID,"images/errocartao.bmp")`.
4. `CHAR` (teclado): `ReceiveCHAR` → mensagem msq tipo 5 para `Menu_504`.

**Timeouts/estados**: não há máquina de estados no servidor — o terminal é que
gerencia seus timeouts de exibição (`Config.time`, tempos por imagem em
`images.txt`/`images.cfg`). O servidor só é reativo. Esperas fixas: `sleep(5)`
entre imagens casadas; `sleep(1)` entre pedir e ler configs (`GET_CONFIG`).
Falha de busca = apenas resposta "não encontrado"; não há retry.

### 1.3 Cadastro de terminais (`tables/names.txt`)

- A lib registra automaticamente IP, ID e tipo na `TabConec` em memória (nada é
  persistido). O comando `tab` do `server` lista POS/IP/tipo.
- O `server` opcionalmente (comando `names`, alterna flag) resolve o IP para um
  **apelido** lendo `tables/names.txt` no formato `IP|nome|` — busca linear,
  compara os **13 primeiros caracteres** (`strncmp(line,IP,13)`), ou seja,
  assume IPv4 em decimal pontuado. Se não achar, mostra o IP.
- `TerminalName()` (server.c:135) traduz o identificador da lib para o nome
  comercial: `tc501`→TC501; `tc502 x`→TC502 / TC502 (TC505) (v2.0) /
  Busca Preço (v2.1); `tc505`→TC505.

### 1.4 Loop de publicidade / imagens (resumo)

- **TC505/TC502 2.0**: ao conectar, recebem a lista completa
  (`SendImageList`, acima). Loop controlado pelo próprio terminal.
- **TC504**: o admin empacota playlists via `server`:
  - `createadv504` cria `adv/<nome>.cfg` com `<`, linhas
    `|arquivo.bmp|tempo|qtd|` e `>`; `sendadv` envia STOP_LOOP, tela
  `UPDADV.BMP`, DELETE_ADV, cada BMP (via `SendFile` para `c:\images\`),
  o cfg para `c:\APPS\images.cfg` e RELOAD_ADV.
  - `createpre504` cria `preimg/<nome>.CFG` (lista de BMPs entre `<`/`>`);
    `sendpre` envia para `c:\preimg\` + RELOAD_PRE (pré-imagens exibidas antes
    do resultado da consulta).
  - `createadv505` recria `images/images.txt` (`arquivo|tempo|5|`), usado no
    connect do TC505.
- `SendImage` (images.c:151) só suporta BMP com paleta (1/4/8 bpp; 24 bpp é
  aceito no header mas não enviado); raster esperado 320 px/linha, buffer
  76800 bytes, paleta convertida para 768 bytes RGB.

### 1.5 Comandos admin do `server` (Process, server.c:1984)

| Comando | Ação |
|---|---|
| `tab` | Pede e exibe tabela de terminais conectados (POS, IP/apelido, tipo) |
| `names` | Liga/desliga resolução de apelidos via `tables/names.txt` |
| `stopserver` | Encerra o daemon TCgertec (CMD STOP; loga STOP_SERVER e `exit(0)`) |
| `sendtext[:...]` | Envia 2 linhas + tempo (TC501) ou texto gráfico com X,Y,cores (TC504) |
| `sendimage[:...]` | Envia BMP do `images/` (+ tempo de exibição no TC501/505) |
| `getconfig` | Lê e exibe Config/UPDConfig/PARAMConfig (TC501) ou Setup TCP (TC504) |
| `setlec[:...]` | Habilita/desabilita leitor de cartão do TC504 |
| `setkey[:...]` | Habilita/desabilita teclado do TC504 |
| `settime[:...]` | Tempo de exibição de telas (TC504) |
| `restart[:...]` | Reinicia terminal (qualquer tipo) |
| `cleardisp[:...]` | Limpa display com cor (TC504) |
| `sendadv[:...]` | Empurra playlist de propagandas `adv/*.cfg` (TC504) |
| `createadv504` / `createadv505` / `createpre504` | Geradores de playlists (§1.4) |
| `sendpre[:...]` | Empurra pré-imagens `preimg/*.CFG` (TC504) |
| `sendconfig` | Grava Config no terminal TC501 (host, endereço, máscara, linhas idle 1-4, tempo) |
| `sendupd` | Grava UPDConfig no TC501 (gateway, DNS, nome, servidor/usuário/senha de atualização) |
| `sendparam` | Grava PARAMConfig no TC501 (IP dinâmico, busca de servidor) |
| `sendsetup` | Grava ARG_SETUP_TCP no TC504 (IP, servidor, máscara, gateway, DNS, nome, porta, FTP, DHCP) |
| `changelanguage` | Troca idioma do CLI (`languages/CONFIG.TXT` → `languages/<arquivo>`) |
| `clear` | `system("clear")` |
| `exit` | Sai do CLI (daemon continua) |

Sufixo `:a` = todos os terminais conectados; `:p1;p2;...` = lista de posições
da `TabConec` (função `Posicoes`, server.c:381). O CLI valida por tipo:
alguns comandos só existem para TC501 (`TerminalError` se type≠0) ou TC504.

---

## 2. Schema SQL usado (tabelas_Mysql.c / tabelas_PostGre.c)

Constantes (`tabelas_Mysql.h`): banco `database`, usuário `user`/`pass`,
tabelas **`pricetable`** e **`cardtable`**. Queries literais usadas:

```sql
-- preço (barcode sem aspas: coluna numérica ou código interpretado como número)
SELECT * FROM pricetable WHERE barcode = <barras>;
-- cartão (MySQL usa aspas; PostGre monta sem aspas)
SELECT * FROM cardtable WHERE cardcode = "<trilhas>";
```

Ordem das colunas inferida dos índices lidos (`linha[n]` / `PQgetvalue(res,0,n)`):

- **pricetable**: `0 barcode, 1 nome, 2 preco, 3 Xnome, 4 Ynome, 5 Xpreco,
  6 Ypreco, 7 cor, 8 background, 9 imagem1, 10 imagem2, 11 imagem3`
  — `imagem1..3` aceitam NULL; `X/Y` são inteiros via `atoi`.
- **cardtable**: `0 cardcode, 1 nome, 2 bonus, 3 imagem` — `imagem` aceita NULL.
- **preco**: STRING (ex.: `"2,35"` com vírgula decimal, até 20 chars) — nunca é
  tratado como número no código; formatado pelo ERP que popula a tabela.
- **Observação**: o PostGre (tabelas_PostGre.c:84) contém um bug clássico:
  `prod.Xpreco=atoi(PQgetvalue(res,0,6))` onde deveria ser `Ypreco` — `Ypreco`
  fica sempre 0. Manter isto em mente ao comparar comportamentos no port.
- Nomes/IPs de terminais **não** vão para o banco no legado — ficam em
  `tables/names.txt` e na `TabConec` em memória.
- Não há tabela de imagens no SQL: os campos `imagem1..3`/`imagem` guardam só o
  **nome do arquivo BMP**, procurado em `images/` no servidor.

### 2.1 DDL equivalente do legado (reconstrução fiel)

```sql
CREATE TABLE pricetable (
  barcode    VARCHAR(20)  NOT NULL,   -- na prática comparado sem aspas (numérico)
  nome       VARCHAR(50),
  preco      VARCHAR(20),             -- "2,35" (vírgula decimal)
  Xnome      INT, Ynome      INT,     -- posição do texto do nome no display TC504
  Xpreco     INT, Ypreco     INT,     -- posição do texto do preço no display TC504
  cor        VARCHAR(10),             -- nome da cor do texto (§4.2)
  background VARCHAR(10),             -- nome da cor de fundo
  imagem1    VARCHAR(255) NULL,       -- BMP principal
  imagem2    VARCHAR(255) NULL,       -- imagem casada 2 (5 s depois)
  imagem3    VARCHAR(255) NULL        -- imagem casada 3 (10 s depois)
);
-- MySQL: ALTER TABLE pricetable ADD PRIMARY KEY (barcode);

CREATE TABLE cardtable (
  cardcode VARCHAR(100) NOT NULL,     -- track1+track2 concatenadas do cartão
  nome     VARCHAR(50),
  bonus    VARCHAR(20),
  imagem   VARCHAR(255) NULL
);
```

### 2.2 Schema SQL recomendado para o port (MySQL/PostgreSQL compatível)

```sql
-- 1. Produtos/preços (uma linha por código de barras)
CREATE TABLE pricetable (
  barcode     VARCHAR(32)  NOT NULL PRIMARY KEY,
  name        VARCHAR(50)  NOT NULL,
  price       NUMERIC(10,2) NOT NULL,        -- recomendação: guardar numérico e
                                             -- formatar com vírgula no protocolo
  name_x      SMALLINT NOT NULL DEFAULT 10,  -- Xnome
  name_y      SMALLINT NOT NULL DEFAULT 10,  -- Ynome
  price_x     SMALLINT NOT NULL DEFAULT 50,  -- Xpreco
  price_y     SMALLINT NOT NULL DEFAULT 30,  -- Ypreco
  color       VARCHAR(10)  NOT NULL DEFAULT 'black',   -- texto
  bg_color    VARCHAR(10)  NOT NULL DEFAULT 'black',   -- fundo
  image1      VARCHAR(255) NULL,             -- BMP em images/
  image2      VARCHAR(255) NULL,
  image3      VARCHAR(255) NULL
);

-- 2. Cartões de bônus/fidelidade (MCR)
CREATE TABLE cardtable (
  cardcode VARCHAR(128) NOT NULL PRIMARY KEY,  -- track1||track2
  name     VARCHAR(50)  NOT NULL,
  bonus    VARCHAR(20)  NOT NULL,
  image    VARCHAR(255) NULL
);

-- 3. Terminais cadastrados (substitui tables/names.txt; apelido por IP)
CREATE TABLE terminals (
  ip       INET/VARCHAR(15) NOT NULL PRIMARY KEY,  -- INET no PG, VARCHAR(15) no MySQL
  name     VARCHAR(32) NOT NULL,
  kind     VARCHAR(8)  NOT NULL DEFAULT 'text'     -- 'text' (TC501/502/505) | 'gfx' (TC504)
);

-- 4. Playlist de publicidade (opcional; substitui images/images.txt e adv/*.cfg)
CREATE TABLE adv_playlist (
  position  SMALLINT NOT NULL,
  image     VARCHAR(255) NOT NULL,   -- arquivo BMP em images/
  show_secs SMALLINT NOT NULL DEFAULT 5,  -- "tempo"
  loops     SMALLINT NOT NULL DEFAULT 5,  -- "qtd/loop"
  PRIMARY KEY (position)
);
```

Notas de port:

- Query equivalente: `SELECT ... FROM pricetable WHERE barcode = $1` (prepared
  statement; o legado concatenava a string — SQL injection possível, corrigir
  no port).
- Comparação do legado é por `strcmp` exato (TXT) e `=` SQL; sem normalização
  de EAN/padding.
- O struct C `Preco` limita: nome 50, preço 20, cores 10, imagens 255 chars.

---

## 3. Formato dos arquivos TXT (backend padrão)

Todos são arquivos de texto, uma linha por registro, campos separados por `|`,
com um `|` final (trailing) antes do `\n`. Parser (tabelas.c) lê até 0xff chars
por linha e compara o 1º campo com `strcmp` exato — busca linear O(n), sem
índice, arquivo reaberto a cada consulta.

### 3.1 `tables/pricetab.txt`

```
<barcode>|<nome>|<preço>|<Xnome>|<Ynome>|<Xpreco>|<Ypreco>|<cor>|<fundo>|<imagem1>|<imagem2>|<imagem3>|
```

Exemplo real:

```
1112223331119|Fritas|2,35|10|10|50|30|red|black|FRIES.bmp|server.bmp||
```

- 12 campos; `imagem2`/`imagem3` podem estar vazios (linha termina `||`).
- `preço` é string livre com vírgula decimal; `X/Y` inteiros (pixels);
  `cor`/`fundo` são nomes da tabela §4.2; imagens são nomes de arquivos BMP em
  `images/`.

### 3.2 `tables/cardtable.txt`

```
<código do cartão>|<nome do cliente>|<bônus>|<imagem>|
```

Exemplo real (código = track1+track2 concatenadas do cartão magnético):

```
111111111111111111111111111111111111111111111111111111111111111111222222222222222222222222222|Cliente preferencial|1000|bonus.bmp|
```

### 3.3 `tables/names.txt`

```
<IP>|<apelido do terminal>|
```

Exemplo real:

```
192.168.0.183|Terminal 5|
192.168.0.184|Terminal 7|
```

Usado apenas pelo CLI `server` (comando `names`). Comparação nos 13 primeiros
caracteres (assume IPv4 xxx.xxx.xxx.xxx).

### 3.4 Arquivos auxiliares de imagem/playlist

- `images/images.txt` (TC505): `arquivo.bmp|tempo|5|` por linha; enviado ao
  terminal conectar (`SendImageList`).
- `adv/<nome>.cfg` (TC504): delimitado por `<` e `>`; linhas internas
  `|arquivo.bmp|tempo|qtd|`; enviado para `c:\APPS\images.cfg`.
- `preimg/<nome>.CFG` (TC504): `<`/`>` com um `arquivo.bmp` por linha; enviado
  para `c:\preimg\` + `c:\APPS\PREIMG.CFG`.
- `languages/CONFIG.TXT`: `nome do idioma|arquivo|` por linha; os arquivos de
  idioma alimentam o struct `TEXT` do CLI.

---

## 4. Parâmetros de configuração

### 4.1 Display idle e tempos (struct `Config`, TC501 — comando `sendconfig`)

| Campo | Significado |
|---|---|
| `host`, `address`, `netMask` | rede do terminal (strings) |
| `line1..line4` (21 chars cada) | 4 linhas exibidas em idle (modo "Busca Preço") |
| `time` (int) | tempo de exibição da resposta (nome+preço) antes de voltar ao idle |

TC504 (`ARG_SETUP_TCP` / `ARG_DISPLAY_TEXT`): texto gráfico com `wPosX`,
`wPosY`, `sText[128]`, `sFont[32]` (`c:\fonts\LUCIDA12.BMP`/`LUCIDA16.BMP` no
terminal), `wSize`, `wColor`, `wBGColor`. Tempos fixos no protocolo 504:
imagens casadas a cada 5 s; playlist com `tempo`/`qtd` por imagem;
`settime` ajusta o tempo de exibição de telas.

### 4.2 Cores — códigos 0x100–0x10F (`ConvertCor`, TCgertec.c:262)

Cores no estilo HTML básico; nomes aceitos nos TXT/SQL e convertidos para o
código de 16 bits enviado ao terminal. Desconhecido ⇒ `0x100` (black).

| Código | Nome | | Código | Nome |
|---|---|---|---|---|
| 0x100 | black | | 0x108 | silver |
| 0x101 | brown | | 0x109 | red |
| 0x102 | green | | 0x10A | lime |
| 0x103 | olive | | 0x10B | yellow |
| 0x104 | navy | | 0x10C | blue |
| 0x105 | purple | | 0x10D | fuchsia |
| 0x106 | teal | | 0x10E | aqua |
| 0x107 | gray | | 0x10F | white |

### 4.3 Outros parâmetros relevantes

- `MAX_TC = 200` terminais simultâneos; `MSG_KEY = 10`; perms msq `0600`.
- Comandos numéricos do protocolo (lib): `BARCODE=89`, `MCR=69`, `CHAR=53`;
  acks: `TEXT_OK=36`, `DISP_CLEAR_OK=34`, `IMAGE_OK=38`, `SET_TIME_OK=40`,
  `LOOP_START=44`, `LOOP_STOP=46`, `SET_KEY=50`, `CHAR=53`, `SET_LEC=66`,
  `MCR=69`, `SEND_FILE_OK=100`, `DELETE_OK=102`, `BARCODE=89`,
  `RELOAD_OK=118`, `RELOAD_PRE_OK=120`, `RESTART_OK=122`,
  `SENDSETUP_OK=24` (msq.h / libTCgertec.h:75-91).
- Imagens TC504: BMP paletado, paleta convertida para 768 bytes RGB, raster
  invertido verticalmente, 320 bytes/linha, buffer 76800 bytes.
- Fontes de referência: `TCgertec.c:451,594` (chamadas SeekBarCode),
  `TCgertec.c:262` (cores), `images.c:99,151`, `server.c:1984` (Process),
  `tabelas.c:50,192` (parsers TXT), `tabelas_Mysql.c:53,130` (queries).

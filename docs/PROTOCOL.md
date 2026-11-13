# Protocolo TCP Gertec Busca-Preço (TC501) — Especificação

Documento gerado por engenharia reversa dos seguintes artefatos legados da Gertec:

| Artefato | Papel |
|---|---|
| `Emu_Buscapreco.jar` → `Terminal.java`, `Controle_Display.java`, `Configuracoes.java`, `CodBarras.java` | Emulador oficial do terminal = **implementação de referência do cliente TCP** (protocolo TC501) |
| `libTCgertec.h`, `msq.h` | Headers da biblioteca do servidor (`libTCgertec.so`) — códigos de comando/ACK binários e structs |
| `TCgertec.c` | Daemon C que usa a lib — mostra como o servidor trata o que recebe dos terminais |

**Público-alvo**: reimplementação do servidor (e/ou cliente) em Rust no crate `buscapreco-rs`.

---

## 1. Visão geral

- **Transporte**: TCP, fluxo contínuo de bytes (sem length-prefix binário no protocolo TC501).
- **Quem conecta**: o **terminal** é sempre o cliente; ele conecta no servidor na **porta `6500`** (valor fixo, hardcoded em `Terminal.java:75` — `new Socket(conf.eip_serv.getText(), 6500)`).
- **Codificação**: ASCII puro (bytes dos caracteres, `String.getBytes()` — efetivamente ASCII/latin-1).
- **Delimitador de comando**: caractere **`#` (0x23)**. Todo comando começa com `#`. Vários comandos podem chegar **coalescidos num mesmo segmento TCP**; o parsing usa a posição do `#` seguinte para separar (função `GetComando`, `Terminal.java:53-64`).
- **Sem terminador de linha**: comandos não terminam com `\r`/`\n` (o `\n` que aparece no código é só log da GUI).
- **Timeout**: `setSoTimeout(120000)` → se o terminal ficar 120 s sem receber nada do servidor, a conexão cai e ele reconecta (a menos que esteja em `alwayslive`).
- **Buffer de leitura**: leituras de até **7000 bytes** por chamada (`readFromSocket`, `Terminal.java:520`).

> O produto real TC501 usa, além deste protocolo ASCII de texto/preço, um protocolo binário interno encapsulado pela `libTCgertec.so` (códigos `TEXT_OK=36`, `IMAGE_OK=38` etc., ver §8). O daemon C só enxerga o lado ASCII; os códigos binários são terminados pela lib no firmware.

---

## 2. Handshake (conexão inicial)

O handshake é iniciado **pelo servidor**:

```
Terminal                          Servidor (porta 6500)
   |---- TCP CONNECT ----------------->|
   |                                   |
   |<---- "#ok" -----------------------|   (servidor envia primeiro)
   |---- "#TC502|2.10" -------------->|   (identificação: modelo|firmware)
   |                                   |
   [sessão estabelecida; display do terminal mostra "Conectado"]
```

Detalhes do código (`Terminal.java:81-89`):

- O terminal lê o primeiro pacote e confere `substring(0,3).equalsIgnoreCase("#ok")` — ou seja, aceita `#OK`/`#ok`.
- Responde com a string de identificação. No emulador é **`#TC502|2.10`** (modelo `TC502`, firmware `2.10`). O terminal real TC501 envia seu próprio modelo/versão no mesmo formato `#<MODELO>|<VERSAO>`.
- Em caso de falha de conexão/timeout, o terminal exibe "Procurando <IP>" e refaz o loop de conexão.

Logo após, o servidor tipicamente interroga a configuração do terminal (`#config?`, `#extconfig?`, ...) — ver §5.

---

## 3. Formato dos frames

### 3.1 Frame genérico

```text
'#' comando [args...] ['#' próximo-comando ...]
```

- `comando`: nome alfanumérico minúsculo (ex.: `mesg`, `nfound`, `config`).
- **Campos de comprimento variável** usam prefixo de tamanho de **1 caractere ASCII**: o comprimento é o próprio caractere menos `'0'` (0x30) — ou seja, campos de até 9 bytes (função usada em todo `Terminal.java`: `(char)(texto.length() + 48)`).

```rust
/// Pseudo-Rust: decodificação de campo com prefixo de tamanho ASCII
fn read_field(buf: &[u8], pos: &mut usize) -> &str {
    let len = (buf[*pos] - b'0') as usize; // 1..=9
    *pos += 1;
    let s = std::str::from_utf8(&buf[*pos .. *pos + len]).unwrap();
    *pos += len;
    s
}
```

- **Campos numéricos de 1 dígito** são enviados como `(char)(valor + 48)` — o dígito em ASCII (ex.: tempo de exibição `5` → byte `0x35`).
- **Campos booleanos** são `'1'` (0x31) = verdadeiro / `'0'` (0x30) = falso.
- **Endianness**: irrelevante no protocolo ASCII. Nas structs binárias da lib (§8), tudo é little-endian (x86, `#pragma pack(1)`).

### 3.2 Coalescência de comandos

`GetComando(tamanho)` verifica se `buf[tamanho] == '#'`; se sim, o restante do buffer é tratado como novo comando dentro do mesmo `read()`. **O servidor pode enviar vários comandos concatenados** e o terminal processa em loop. O emulador, porém, encerra o processamento do bloco na maioria dos handlers (`fim_mensagem = true`), comportamento conservador — o servidor legado envia um comando por pacote na prática.

### 3.3 Máquina de estados do terminal (estados relevantes)

- `barras_ok` (0/1): terminal enviou código de barras e aguarda resposta de preço. **Enquanto `barras_ok == 1`, comandos de configuração são ignorados**; só são aceitos `#nfound` e a resposta de preço (`#...|...`).

---

## 4. Comandos servidor → terminal (protocolo ASCII TC501)

| Comando (wire) | Formato dos argumentos | Resposta do terminal | Efeito | Fonte |
|---|---|---|---|---|
| `#ok` | — | `#TC502\|2.10` | Handshake | Terminal.java:82-83 |
| `#live?` | — | `#live` | Keep-alive; servidor deve enviar antes do timeout de 120 s | Terminal.java:96-109 |
| `#alwayslive` | — | `#alwayslive_ok` | Desativa timeout (setSoTimeout(0)); conexão permanente | Terminal.java:111-117 |
| `#checklive` | — | `#checklive_ok` | Reativa timeout de 120 s | Terminal.java:118-133 |
| `#config?` | — | `#config...` (ver §5.1) | Pede configuração básica | Terminal.java:134-140 |
| `#config02?` | — | `#config02...` | Pede configuração 4 linhas | Terminal.java:141-147 |
| `#extconfig?` | — | `#extconfig...` | Pede configuração estendida | Terminal.java:148-154 |
| `#paramconfig?` | — | `#paramconfig...` | Pede parâmetros (2 flags) | Terminal.java:155-161 |
| `#updconfig?` | — | `#updconfig...` | Pede config de atualização (FTP) | Terminal.java:162-168 |
| `#mesg` | `L1len L1 L2len L2 TT` | — (mostra na hora) | **Mensagem instantânea**: 2 linhas (até 9 bytes cada no formato, 20 no terminal real) + tempo de exibição (2 dígitos ASCII; emulador usa só o 1º = unidades de segundo) | Terminal.java:389-415 |
| `#rconf` | len-prefixed: ipServ, ipTerm, netMask, l1, l2, tempo(1 dígito) | — (salva config) | Reconfiguração básica (grava e reinicia display) | Terminal.java:190-225 |
| `#reconf02` | idem + l3, l4 (8 campos + tempo) | — | Reconfiguração 4 linhas (`trataReconf02`) | Terminal.java:226-267 |
| `#rextconf` | ipServ, ipTerm, netMask, gateway, nomeServ, nomeTerm, l1, l2, endUpdate, user, pass, tempo, ipDinamico(0/1), buscaServ(0/1) — todos len-prefixed; tempo como dígito ASCII | `#rextconf_ok` | Reconfiguração estendida | Terminal.java:268-328 |
| `#rparamconfig` | 2 bytes fixos `0`/`1`: ipDinamico, buscaServ | `#rparamconfig_ok` | Reconfigura parâmetros | Terminal.java:329-352 |
| `#rupdconfig` | gateway, nomeServ, nomeTerm, endUpdate, user, pass (len-prefixed) | — | Reconfigura atualização | Terminal.java:353-388 |
| `#restartsoft` | 4 bytes mágicos nas posições 12–15: **`0xA5 0xCC 0x5A 0x33`** | `#restartsoft_ok` e fecha a conexão | Reinício controlado; sem a assinatura, o terminal ignora com erro | Terminal.java:169-189 |
| `#nfound` | — (só válido com `barras_ok==1`) | — (display "não encontrado") | Produto não encontrado | Terminal.java:416-426 |
| `#<desc>\|<preco>` | descrição `\|` (`0x7C`) preço; preço pode ter sufixo `#...` (descartado por `trataPrice`) | — (display preço) | **Resposta de preço** após leitura de barras | Terminal.java:427-454 |

Observações:

- **`#mesg` tempo**: `Terminal.java:398-399` lê dois dígitos (`t` e `zero`), mas só usa `t`. Formato no wire: 2 bytes ASCII de dígito. Se precisar de tempos ≥ 10 s, testar contra firmware real (provável BCD: dezena+unidade).
- **`#restartsoft`**: o código checa `bytes[12..15] == A5 CC 5A 33` com `length() >= 15`; o frame é `#restartsoft` (12 bytes, posições 0–11) seguido **imediatamente** da assinatura `A5 CC 5A 33` (posições 12–15) — 16 bytes no total, **sem padding** (verificado em `Terminal.java:169-189`).
- Quaisquer outros comandos com `barras_ok==0` caem em "comando não reconhecido" e são descartados sem resposta.

## 5. Comandos terminal → servidor

### 5.1 Respostas de configuração (ao `#...?`)

Formato: comando (sem `?`) + sequência de campos len-prefixed.

**`#config`** (`Terminal.java:136`):

```
"#config" L(ipServ) ipServ L(ipTerm) ipTerm L(netMask) netMask L(l1) l1 L(l2) l2 tempo
```
onde `tempo` é **o texto** configurado (ex.: `"5"`), sem prefixo.

**`#config02`** (`Terminal.java:143`): idem, com `l1..l4` e tempo como `(char)(valor+48)` (1 dígito).

**`#extconfig`** (`Terminal.java:150`): `ipServ, ipTerm, netMask, gateway, nomeServ, nomeTerm, l1, l2, endUpdate, user, pass` (len-prefixed) + tempo (dígito ASCII) + ipDinamico(0/1) + buscaServ(0/1).

**`#paramconfig`** (`Terminal.java:157`): `#paramconfig` + 2 bytes `0`/`1` (ipDinamico, buscaServ).

**`#updconfig`** (`Terminal.java:164`): `gateway, nomeServ, nomeTerm, endUpdate, user, pass` (len-prefixed).

### 5.2 Código de barras (`sendBarras`)

```
Terminal                          Servidor
   |---- "#7891234567890" ----------->|   (sendBarras; '#' + dígitos do código)
   |        [barras_ok = 1]           |
   |<--- "#ARROZ 5KG|3,99" -----------|   produto encontrado
   |        ou                        |
   |<--- "#nfound" -------------------|   não encontrado
```

- `CodBarras.java:37` prependa o `#`; `sendBarras` (`Terminal.java:536-545`) envia cru, sem framing adicional. Códigos com menos de 2 caracteres são descartados localmente.
- O daemon (`TCgertec.c:584-607`, `Check()`) recebe o buffer via `CheckCMD`, ignora `#img_ok`/`#img_error`/`#restartsoft_ok`, e trata qualquer `#<barras>` como consulta: procura no bd e responde `SendProductPrice(ID, nome, preco)` → no wire vira `#<nome>|<preco>`; se não achar, `SendProductNotFound(ID)` → `#nfound`.

### 5.3 ACKs observados do terminal (protocolo ASCII)

| String | Contexto |
|---|---|
| `#TC502\|2.10` | identificação pós-`#ok` |
| `#live` | resposta a `#live?` |
| `#alwayslive_ok` | resposta a `#alwayslive` |
| `#checklive_ok` | resposta a `#checklive` |
| `#rextconf_ok` | resposta a `#rextconf` |
| `#rparamconfig_ok` | resposta a `#rparamconfig` |
| `#restartsoft_ok` | resposta a `#restartsoft` válido (fecha em seguida) |
| `#img_ok` / `#img_error` | ACKs de imagem (firmware real com suporte a imagens; o daemon os filtra em `Check()`) |

---

## 6. Como chega a resposta de preço / product-not-found

`Terminal.java:427-454` — com `barras_ok == 1`, **qualquer frame iniciado por `#`** que não seja `#nfound` é tentado como preço:

1. `pospipe = indexOf('|')` (0x7C);
2. `desc = frame[1..pospipe)`, `preco = frame[pospipe+1..]`;
3. `trataPrice(preco)`: se houver `#` no preço, trunca no primeiro `#` (permite coalescência com o comando seguinte);
4. `barras_ok = 0`;
5. Se o preço resultante for vazio ou começar com `#`: exibe só a descrição; senão exibe descrição + preço por `tempo` segundos (config. `Time`) e volta ao loop de propagandas.

`#nfound`: exibe mensagem localizada de "não encontrado" (2 linhas) e `barras_ok = 0`.

No servidor Rust, a resposta deve ser um único `write()` com `#desc|preco` (sem `\n`), de preferência em um segmento TCP separado (ou concatenada com `#` se houver comando seguinte).

---

## 7. Exemplo de sessão completa (byte a byte)

Configuração do terminal: `IPServer=192.168.0.1`, `IPTerm=192.168.0.20`, `NetMask=255.255.255.0`, `Gateway=192.168.0.1`, `Nome=TC300`, `NameServ=192.168.0.2`, `UpdateAddress=http://www.gertec.com.br`, `Time=5`, `Line1=Busca Preco`, `Line2=Gertec`, `Line3=Passe o produto`, `Line4=no leitor`, `User=gertec`, `Pass=gertec`, flags 0/0 (defaults do `Configuracoes.createConfig()`).

Hexdump (colunas S=servidor→terminal, T=terminal→servidor):

```text
 1. S: 23 6F 6B                                   "#ok"
 2. T: 23 54 43 35 30 32 7C 32 2E 31 30          "#TC502|2.10"
 3. S: 23 65 78 74 63 6F 6E 66 69 67 3F           "#extconfig?"
 4. T: 23 65 78 74 63 6F 6E 66 69 67              "#extconfig"
      39 31 39 32 2E 31 36 38 2E 30 2E 31         "9" "192.168.0.1"        (ipServ)
      39 31 39 32 2E 31 36 38 2E 30 2E 32 30      "9" "192.168.0.20"       (ipTerm)
      39 32 35 35 2E 32 35 35 2E 32 35 35 2E 30   "9" "255.255.255.0"      (netMask)
      39 31 39 32 2E 31 36 38 2E 30 2E 31         "9" "192.168.0.1"        (gateway)
      39 31 39 32 2E 31 36 38 2E 30 2E 32         "9" "192.168.0.2"        (nomeServ)
      35 54 43 33 30 30                           "5" "TC300"              (nomeTerm)
      36 42 75 73 63 61 20 50 72 65 63 6F 63 39   "6" "Busca Preco"  "9"?  (l1 — ver nota)
      ... (l2, endUpdate, user, pass, tempo '5', '0', '0')
    NOTA: comprimentos >9 (ex. endUpdate) não cabem no dígito único; o
    firmware real usa o mesmo esquema (campos limitados); para o exemplo,
    considere campos curtos.
 5. S: 23 6C 69 76 65 3F                          "#live?"           (a cada <120 s)
 6. T: 23 6C 69 76 65                             "#live"
 7. T: 23 37 38 39 31 32 33 34 35 36 37 38 39 30  "#7891234567890"   (leitura de barras)
 8. S: 23 41 52 52 4F 5A 20 35 4B 47 7C 33 2C 39 39 "#ARROZ 5KG|3,99" (resposta de preço)
 9. S: 23 6D 65 73 67 34 4F 4C 41 33 52 45 43 30 35
      "#mesg" "4" "OLÁ" "3" "REC" "05"                              (mensagem instantânea, 5 s)
10. T: 23 37 38 39 39 39 39 39 39 39 39 39        "#789999999999"    (barras inexistente)
11. S: 23 6E 66 6F 75 6E 64                        "#nfound"
12. S: 23 72 65 73 74 61 72 74 73 6F 66 74 ?? ?? A5 CC 5A 33          (#restartsoft + mágico)
13. T: 23 72 65 73 74 61 72 74 73 6F 66 74 5F 6F 6B "#restartsoft_ok" (e fecha/reconecta)
```

Sequência resumida (texto):

```
S→T #ok
T→S #TC502|2.10
S→T #extconfig?
T→S #extconfig9192.168.0.19192.168.0.209255.255.255.0...
S→T #live?            (keepalive periódico)
T→S #live
T→S #7891234567890
S→T #ARROZ 5KG|3,99
S→T #mesg4OLA3REC05
T→S #789999999999
S→T #nfound
S→T #restartsoft??A5CC5A33
T→S #restartsoft_ok   (desconecta e reconecta)
```

---

## 8. Referência: códigos binários da libTCgertec (protocolo interno/lib)

Estes valores não aparecem no tráfego ASCII acima — são usados entre a `libTCgertec.so` e os terminais no protocolo binário (e no 504). Incluídos para referência do port (extraídos de `libTCgertec.h`):

### 8.1 Comandos terminal → servidor (via `CheckCMD`/`CheckCMD_504`)

| Nome | Valor (dec) | Valor (hex) | Significado |
|---|---|---|---|
| `CHAR` | 53 | 0x35 | Tecla pressionada (TC504) |
| `SET_KEY` | 50 | 0x32 | ACK habilita teclado |
| `DISP_CLEAR_OK` | 34 | 0x22 | ACK de limpar display |
| `TEXT_OK` | 36 | 0x24 | ACK de texto |
| `IMAGE_OK` | 38 | 0x26 | ACK de imagem |
| `SET_TIME_OK` | 40 | 0x28 | ACK de ajuste de tempo de exibição |
| `LOOP_START` | 44 | 0x2C | Loop de propagandas iniciado |
| `LOOP_STOP` | 46 | 0x2E | Loop de propagandas parado |
| `SENDSETUP_OK` | 24 | 0x18 | ACK de setup (504) |
| `MCR` | 69 | 0x45 | Leitura de cartão (trilhas 1/2) |
| `SET_LEC` | 66 | 0x42 | ACK configura leitor |
| `BARCODE` | 89 | 0x59 | Código de barras lido |
| `SEND_FILE_OK` | 100 | 0x64 | ACK de arquivo |
| `DELETE_OK` | 102 | 0x66 | ACK de exclusão |
| `RELOAD_OK` | 118 | 0x76 | ACK de reload |
| `RELOAD_PRE_OK` | 120 | 0x78 | ACK reload pré-imagem |
| `RESTART_OK` | 122 | 0x7A | ACK de restart |

### 8.2 Structs binárias (`#pragma pack(1)`, little-endian)

```rust
#[repr(C, packed)] // ARG_SETUP_TCP (libTCgertec.h) — setup do TC504
struct ArgSetupTcp {
    dw_my_ip_add: u32, dw_server_ip: u32, dw_net_mask: u32,
    dw_gateway: u32, dw_name_server: u32,
    tc_name: [u8; 32],
    w_portsv: u16,
    ftps: [u8; 100], ftpu: [u8; 30], ftp_p: [u8; 30],
    dw_dhcp: u32, dw_search_server: u32,
}

#[repr(C, packed)] // ARG_DISPLAY_TEXT — texto no TC504
struct ArgDisplayText {
    w_pos_x: u16, w_pos_y: u16,
    s_text: [u8; 128], s_font: [u8; 32],
    w_size: u16, w_color: u16, w_bg_color: u16,
}

#[repr(C, packed)] // Config — protocolo TC501 (ASCII fields)
struct ConfigTc501 {
    host: [u8; 21], address: [u8; 21], net_mask: [u8; 21],
    line1: [u8; 21], line2: [u8; 21], line3: [u8; 21], line4: [u8; 21],
    time: i32,
}

#[repr(C, packed)] // UPDConfig
struct UpdConfig { gateway: [u8;21], nameserv: [u8;21], tcname: [u8;21],
                   upd_serv: [u8;99], updser: [u8;21], upd_pass: [u8;21] }

#[repr(C, packed)] // PARAMConfig
struct ParamConfig { ip_dinamico: i8, busca_server: i8 }
```

### 8.3 Cores (u16, `ConvertCor` em `TCgertec.c`)

`black=0x100, brown=0x101, green=0x102, olive=0x103, navy=0x104, purple=0x105, teal=0x106, gray=0x107, silver=0x108, red=0x109, lime=0x10A, yellow=0x10B, blue=0x10C, fuchsia=0x10D, aqua=0x10E, white=0x10F`

### 8.4 Tabela de terminais (lib, `TabConec`)

Até **200 terminais** (`MAX_TC`); por terminal: `ID` (i32), `IP` (u32, ordem da lib), `type` (i32: **0 = TC501/protocolo ASCII-texto, 1 = TC504/binário**), `name[20]`.

### 8.5 Interface SysV message queue (daemon ↔ aplicações servidor, `msq.h`)

Chave `MSG_KEY = 10`, permissão `0600`. `MSG_DATA { long type; int CMD; int ID; char data[7000]; }` com comandos `STOP=0, SEND_TEXT=1, SEND_IMAGE=2, TAB_CONEC=3, GET_CONFIG=4, S_LEC=5, S_KEY=6, S_TIME=7, RESTART=8, CLEAR=9, START_LOOP=10, STOP_LOOP=11, SEND_FILE=12, RELOAD_ADV=13, DELETE_ADV=14, RELOAD_PRE=15, DELETE_PRE=16, SEND_CONFIG=17, SEND_UPD=18, SEND_PARAM=19, SEND_SETUP=20`. No port Rust, substituir por canal interno (tokio mpsc).

---

## 9. Regras de interoperabilidade para o servidor Rust (checklist)

1. Escutar em **0.0.0.0:6500**; ao aceitar conexão, enviar imediatamente **`#ok`**.
2. Aceitar a identificação **`#TCnnn|versao`**; registrar tipo (TC50x) e versão.
3. Manter keepalive: enviar **`#live?`** com período < 120 s; ou negociar **`#alwayslive`**.
4. Interrogar configuração com `#extconfig?` (ou `#config?`) no momento desejado; parsear prefixos de tamanho ASCII.
5. Ao receber **`#<barras>`** (não-ACK): consultar bd e responder **`#<desc>|<preco>`** ou **`#nfound`**, em um único `write`.
6. Ignorar/respeitar ACKs `#img_ok`, `#img_error`, `#restartsoft_ok`, `#rextconf_ok`, `#rparamconfig_ok`, `#live`, `#alwayslive_ok`, `#checklive_ok`.
7. Nunca concatenar resposta de preço com outro comando sem `#` separador; campos len-prefixed limitados a 9 bytes com 1 dígito ASCII (validar!).
8. Para reinício controlado: **`#restartsoft` + 2 bytes padding + `A5 CC 5A 33`**; aguardar `#restartsoft_ok` e fechar.

---

*Fontes primárias: `Terminal.java` (descompilado CFR), `Controle_Display.java`, `Configuracoes.java`, `CodBarras.java`, `libTCgertec.h`, `msq.h`, `TCgertec.c`. Emulador: Emu_Buscapreco.jar (rev. 10/2007, Pedro Roberto S. Junior, Gertec).*

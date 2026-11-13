# Notas Técnicas — Gertec TC505 (Emulador) e Kit Busca Preço

Fontes analisadas:
- `Emu_TC505.pdf` — Manual do Usuário do Emulador TC505 (Gertec, REV. 00, 10/2007, Pedro Roberto S. Junior)
- `Emu_Buscapreco.pdf` — Manual do Usuário do Emulador BuscaPreço (Gertec, REV. 01, 10/2007, mesmo autor)
- `Emu_TC505.jar` — descompilado com CFR para `/tmp/opencode/re/decomp_505` (pacote `br.com.gertec.tc505.emulador`)
- `config.txt` — configuração persistida pelo emulador

## 1. Resumo dos PDFs

Ambos os PDFs são **manuais de usuário dos emuladores**, não especificações de protocolo:

- Descrevem instalação (JRE 1.5+/1.6+, resolução 1024x768), a interface (menu, display, leitor de barras simulado, painel de mensagens/protocolo, painel de configuração) e o fluxo de uso: configurar → Salvar → Terminal → Iniciar → terminal conecta ao servidor e exibe propagandas → enviar código de barras → display mostra nome do produto e preço, ou "Produto não Cadastrado".
- O painel "Mensagens" mostra a comunicação terminal↔servidor (útil como log de protocolo), mas **nenhum comando, porta ou formato de frame é documentado nos PDFs**.
- Troubleshooting genérico: verificar configurações, firewall, versão do JRE.

**Conclusão: todo o conhecimento de protocolo vem da engenharia reversa do JAR (abaixo).**

## 2. Protocolo TC505 (extraído do bytecode)

Classe central: `Terminal.java` (`br.com.gertec.tc505.emulador`).

### 2.1 Conexão e handshake

- **Transporte**: TCP, terminal é **cliente**; conecta a `IPServer:6500`.
- **Handshake**:
  1. Servidor envia `#ok` (terminal valida os 3 primeiros bytes, case-insensitive).
  2. Terminal responde com identificação:
     - `#tc505|1.20` — se a opção **Server505** estiver marcada (`Server505=1` no config.txt)
     - `#tc502|2.0` — caso contrário (modo legado TC502)
- Socket timeout de 120 s (removido com `#alwayslive`).
- Loop principal: lê bytes até encontrar `#` (0x35... na verdade byte 35 decimal = `'#'`), depois dispatch por prefixo de 4–13 chars via máquina de estados (`controle` 1→12).

### 2.2 Comandos servidor → terminal

Todos os frames começam com `#` e são ASCII, exceto o payload binário de `#img`.

| Comando | Formato / comportamento | Resposta do terminal |
|---|---|---|
| `#img` + header | Header ASCII-hex de 17 bytes (posições 4–20): índice (2), nloops (2), tempo (2), nquadros (2), tamanho (2+2, deslocado 4 bits), checksum (2+2), ETB no byte 20. Depois vêm `1026 × nquadros` bytes binários da imagem. Índice `FF` = limpar buffer. | `#img_ok00` |
| `#mesg` | Texto: `len1` (1 dígito ASCII) + linha1 + `len2` + linha2 + `tempo` (1 dígito) | — (exibe no display) |
| Resposta de preço | `#<descrição>\|<preço>` (lê até 40 bytes; pipe separa desc/preço; preço pode ter `#` como terminador extra via `trataPrice`) | exibe descrição + preço |
| `#nfound` | Produto não cadastrado | exibe msg "não encontrado" |
| `#live?` | Ping | `#live` |
| `#checklive` | Verificação de atividade (renova timeout 120 s) | `#checklive_ok` |
| `#alwayslive` | Modo sempre ativo (timeout = 0) | `#alwayslive_ok` |
| `#config?` | Pede config básica | `#config` + campos |
| `#config02?` | Pede config com 4 linhas | `#config02` + campos |
| `#reconf02` | Recebe config nova (mesmo formato de `#config02`); terminal reinicia conexão | — |
| `#extconfig?` | Pede config estendida | `#extconfig` + campos |
| `#updconfig?` | Pede config de update | `#updconfig` + campos |
| `#rupdconfig` | Recebe config de update; terminal reinicia display | — |
| `#paramconfig?` | Pede parâmetros | `#paramconfig` + `IPDinamico`(0/1) + `BuscaServ`(0/1) |
| `#rparamconfig` | Recebe parâmetros; reinicia display | — |
| `#restartsoft` | Reinicia terminal (lê 4 bytes extras) | `#restartsoft_ok` e reconecta |
| `#updatesoft` | Atualização de firmware (ignorada no emulador) | — |
| Outros prefixos aceitos sem tratamento: `#liv`, `#rco`, `#nfo`, `#con`, `#rec`, `#rex`, `#che`, `#alw`, `#upd`, `#ext`, `#rup`, `#res`, `#par`, `#rpa` | — | — |

### 2.3 Codificação dos campos de configuração

Formato de `#config`, `#config02`, `#extconfig`, `#updconfig`: cada campo string é precedido por **1 byte com o comprimento em ASCII** (`len + 48`, ou seja '0'–'9'; campos numéricos/flags idem como char). O parser faz `read() - 48` para recuperar o tamanho.

Campos (na ordem):
- `#config`: ip_serv, ip_term, net_mask, linha1, linha2, tempo_exib
- `#config02`: ip_serv, ip_term, net_mask, linha1..linha4, tempo_exib
- `#extconfig`: ip_serv, ip_term, net_mask, gateway, nome_serv, nome_term, linha1, linha2, end_update, user, pass, tempo_exib, ip_dinamico(0/1), busca_serv(0/1)
- `#updconfig`: gateway, nome_serv, nome_term, end_update, user, pass

### 2.4 Terminal → servidor (sendBarras)

- `sendBarras(codigo)`: escreve **os dígitos do código de barras puros** no socket (sem prefixo, sem sufixo). Comprimento mínimo 2.
- Servidor responde com o frame de preço (`#desc|preco`) ou `#nfound`.

### 2.5 Números no header de `#img`

Campos do header são **hex ASCII** decodificados por `asciiToByte` (`'0'-'9'` → 0–9; `'A'-'F'` → 10–15 via −55). `tamanho` e `cks` usam 4 nibbles com deslocamento de 4 bits (na verdade `(msb<<4)+lsb`, com o primeiro par também deslocado — ver `configuraImagem`, ligeiro quirk do firmware original: `tamanho = asciiToByte(b12,b13) << 4` e depois `+ asciiToByte(b14,b15)`).

## 3. config.txt (config.txt do emulador)

```
IPServer=192.168.0.2        # IP do servidor (porta fixa 6500 no código)
IPTerm=192.168.0.54         # IP do terminal
NetMask=255.255.255.0
Gateway=192.168.0.2
Name=TC505                  # nome do terminal
NameServ=192.168.0.2        # servidor de nomes
UpdateAddress=http://www.gertec.com.br   # endereço de atualização (HTTP)
Time=5                      # tempo de exibição
Line1..Line4=...            # linhas de propaganda padrão
User=gertec / Pass=gertec   # credenciais de update
IPDinamico=0                # flag ip_dinamico (#paramconfig)
BuscaServ=0                 # flag busca_serv
Server505=1                 # MODO 505: muda a identificação do handshake
```

## 4. Diferenças vs TC501 / família TC502

- **Porta**: TC505 usa **TCP 6500** (hardcoded em `Terminal.java:212`). É a mesma porta usada pela família anterior (TC501/TC502) — confirmar no doc do TC501, mas o próprio emulador do TC505 tem o modo legado na mesma porta.
- **Handshake**: a única diferença entre "modos" é a string de identificação: `#tc505|1.20` (Server505=1) vs `#tc502|2.0` (Server505=0). Ou seja, o protocolo de wire é **um só**, com dois perfis de identificação/versão.
- **Efeito prático de Server505=1** no emulador: no tratamento de `#img`, apenas no modo 505 a imagem é efetivamente encaminhada ao display (`sendToDisp`); no modo legado ela é apenas armazenada. Sugere que o servidor 505 espera `#img_ok00` síncrono por quadro (há `Thread.sleep(100)` entre envios).
- **Comandos adicionais do 505** (não presentes no protocolo básico TC501/TC502): `#config02?/#config02/#reconf02` (4 linhas de display), `#extconfig?/#extconfig` (gateway, DNS, endereço de update HTTP com user/senha, IP dinâmico, busca-servidor), `#updconfig?/#updconfig/#rupdconfig`, `#paramconfig?/#paramconfig/#rparamconfig`, `#restartsoft`, `#updatesoft` (update de firmware via `UpdateAddress` — HTTP, não FTP).
- **Formato dos frames**: idêntico ao do TC501 — ASCII com `#`, campos com prefixo de comprimento em dígito ASCII, `#img` com header hex-ASCII + payload binário de 1026 bytes/quadro, respostas `*_ok`.

## 5. Mesmo servidor ou servidor diferente?

O TC505 fala com o **mesmo tipo de servidor** (mesma porta 6500, mesmo formato de frame). O flag `Server505` indica apenas que o servidor conhece o perfil 505 (identificação `#tc505|1.20`) e pode usar os comandos estendidos (`#config02`, `#extconfig`, `#updconfig`, `#restartsoft`, `#updatesoft`). Um servidor que implemente o perfil TC505 atende também terminais legados (eles se identificam como `#tc502|2.0` e o servidor simplesmente não envia os comandos estendidos).

## 6. Recomendação para o port em Rust

**Sim, o port deve suportar o perfil TC505 — e ele cobre o TC501/TC502 com custo marginal quase zero.** Não são dois protocolos, são dois perfis de identificação sobre o mesmo wire format.

Implementação sugerida:

1. **Um único servidor TCP na porta 6500** (configurável).
2. Após aceitar a conexão, enviar `#ok` e aguardar a identificação; guardar o perfil (`tc505|1.20` vs `tc502|2.0`) na sessão.
3. Parser de frames orientado a bytes com máquina de estados (como o `controle` 1→12 do emulador): lê até `#`, acumula prefixo, faz dispatch. Não usar leituras lineizadas — `#img` muda para binário no meio do frame.
4. Tipos de comando em Rust: enum `ServerMsg { Img{indice, nloops, tempo, nquadros, tamanho, cks, data}, Mesg{...}, Price{desc, preco}, NFound, Live, CheckLive, AlwaysLive, Config?, ... }` com serialização dos campos length-prefixed (`len as char` ASCII).
5. Consulta de preço: resposta ao código de barras recebido como dígitos crus = `#<desc>|<preco>`.
6. Para imagem: responder `#img_ok00` após cada quadro e respeitar o atraso ~100 ms (o firmware 505 espera pacing).
7. Atualização de firmware/config via HTTP (`UpdateAddress`, user/pass) — opcional no port; pode ficar como URL servida por outro componente.

Ganchos de código úteis (decompilado): `Terminal.java` (loop de protocolo, `sendBarras`, `trataPrice`, `configuraImagem`), `Configuracoes.java` (ordem dos campos de `#extconfig`/`#updconfig`, leitura/escrita do config.txt).

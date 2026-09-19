# Auditoria nativa do Buzz e da implantação no VPS

Data da auditoria: 19 de setembro de 2026
Buzz auditado: `block/buzz` em `4ab4f786085a23fe6126529861840eff6048ceee`
Relay implantado: imagem `ghcr.io/block/buzz` do mesmo commit

## 1. Critério arquitetural

Esta auditoria segue uma regra simples:

1. identificar o contrato nativo do Buzz;
2. usar configuração e pontos de extensão oficiais;
3. corrigir divergências da nossa implantação;
4. testar o comportamento real com cada harness;
5. criar acoplamento somente para uma lacuna comprovada.

Um acoplamento não deve alterar o relay, o protocolo ou a representação de
agentes quando existe uma extensão oficial. A preferência é por providers,
personas, harness definitions, eventos Nostr e ACP.

## 2. Arquitetura nativa

```text
Buzz Desktop / Mobile
        │
        │ Nostr: mensagens, canais, identidade e controle
        ▼
Buzz Relay
        ├── Postgres: eventos, membros, canais, políticas e auditoria
        ├── Redis: presença, typing e fan-out
        └── S3/MinIO: mídia e Git

Agente no host ou substrato remoto
        │
        ├── buzz-acp: bridge Buzz ↔ ACP
        │       ├── filtro de autores
        │       ├── escopo de sessão
        │       ├── prompt Buzz e contexto da conversa
        │       ├── observer NIP-AO
        │       └── métricas NIP-AM
        │
        └── adapter/harness ACP
                ├── codex-acp → Codex app-server
                ├── claude-agent-acp → Claude Code
                └── opencode acp → OpenCode
```

O relay não executa o modelo. Ele autentica identidades, aplica a fronteira da
comunidade e dos canais, persiste eventos e distribui mensagens. O `buzz-acp`
admite eventos e transforma cada turno em uma sessão ACP. O adapter traduz ACP
para o harness escolhido.

## 3. Capacidades nativas relevantes

| Capacidade | Estado | Implementação |
|---|---|---|
| Identidade de humanos e agentes | Nativo | chave Nostr + assinatura Schnorr |
| Autenticação no relay | Nativo | NIP-42 em WebSocket e NIP-98 em HTTP |
| Comunidades e canais | Nativo | tenant derivado do host + NIP-29 |
| Papéis no canal | Nativo, com ressalva | owner, admin, member, guest; `bot` é designação separada. O ingest genérico ainda não prova `guest` read-only |
| Propriedade de agente | Nativo | NIP-OA; owner atestado criptograficamente |
| Perfil/diretório do agente | Nativo | eventos de perfil e runtime directory |
| Política gerenciada do agente | Nativo | evento owner-signed `kind:30177` |
| Prompt do agente | Nativo, transporte variável | Claude recebe append de system prompt; Codex/OpenCode ACP v1 recebem fallback na primeira mensagem de usuário |
| Harness variável | Nativo | runtime catalog, presets e harness definitions |
| Codex | Tier-1 | `codex-acp` |
| Claude Code | Tier-1 | `claude-agent-acp` |
| OpenCode | Tier-2 oficial | `opencode acp` |
| Sessão compartilhada por canal | Nativo | `BUZZ_ACP_SESSION_POLICY=channel` |
| Sessão isolada por thread | Nativo | `BUZZ_ACP_SESSION_POLICY=thread` |
| DMs | Nativo | sempre uma sessão por conversa DM |
| Memória do agente | Nativo, escopo limitado | core NIP-AE auto-injetado e memória fria on-demand; não compartilha transcript/estado interno entre sessões |
| Atividade do harness | Nativo | NIP-AO `kind:24200` |
| Métricas de uso | Nativo, condicionado | NIP-AM `kind:44200`; exige owner e usage compatível. Tracker agora normaliza Codex, Claude, OpenCode e adapters ACP desconhecidos |
| Modelo persistente | Nativo | definição e instância |
| Effort persistente | Parcial | instância local ou payload do provider; não é campo da persona |
| Troca de modelo ao vivo | Parcial | backend e protocolo existem; controle React não está montado na UI atual |
| Troca de effort ao vivo | Ausente | effort é aplicado no início da sessão |
| Retomada após restart do `buzz-acp` | Lacuna confirmada | mapa de sessões existe somente em memória |
| Principal humano confiável dentro das tools | Lacuna confirmada | prompt recebe pubkey; MCP recebe apenas identidade do agente |
| Visibilidade detalhada para requester não-owner | Lacuna upstream | NIP-AO atual cifra somente para o owner |
| Provider Kubernetes | Nativo | `buzz-backend-kubernetes` |
| Provider systemd/host persistente | Ausente | nosso unit é launcher manual, não provider gerenciado pelo Desktop |

## 4. Harnesses

### 4.1 Matriz comprovada

A matriz abaixo separa o que o Buzz implementa do que cada adapter realmente
anunciou. Codex e Claude foram interrogados localmente por `initialize` e
`session/new` em 19/09/2026. OpenCode não está instalado; sua coluna deriva do
preset do Buzz e da documentação oficial do modo ACP.

| Item | Codex `0.154.0` + `codex-acp 1.12.0` | Claude SDK CLI `2.1.274` + `claude-agent-acp 0.79.0` | OpenCode |
|---|---|---|---|
| Classe no catálogo Buzz | builtin Tier-1 | builtin Tier-1 | preset Tier-2 oficial |
| Comando | `codex-acp` | `claude-agent-acp` | `opencode acp` |
| Estado no host | adapter em `/opt` com Codex embutido; CLI do usuário também instalado | adapter e CLI `2.1.241` no HOME; adapter executa o CLI `2.1.274` embutido no SDK | ausente |
| Runtime isolado `/opt` | pronto | adapter ainda não copiado | ausente |
| ACP negociado | versão 1 | versão 1 | versão 1 documentada |
| Prompt do Buzz | contexto no primeiro bloco de usuário da sessão | `_meta.systemPrompt: {append: ...}` | contexto no primeiro bloco de usuário da sessão |
| Modelo | `configOptions.category=model` e catálogo legado `models` | `configOptions.category=model` | opção `provider/model` |
| Effort | `thought_level`; valores variam por modelo | `thought_level`; catálogo dinâmico | variante do modelo, quando existir |
| Modos ACP | `read-only`, `agent`, `agent-full-access` | `default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions` | IDs dos agentes primários visíveis |
| Política de tools | sandbox/aprovação Codex | modos e permissões Claude | `allow`, `ask`, `deny` por tool/padrão em config |
| MCP fornecido pelo Buzz | `buzz-dev-mcp` é default do builtin; stdio/HTTP aceitos pelo adapter | nenhum default; aceita stdio/HTTP/SSE do cliente | nenhum default; stdio/HTTP; suporte SSE diverge entre docs e source atual |
| Tools próprias | shell, arquivos, busca, skills e demais tools Codex | tools do Claude Agent SDK, skills e subagents | tools, custom tools, plugins, commands, skills e MCP do projeto |
| `session/load` | anunciado e implementado | anunciado e implementado | documentado |
| `session/resume/fork/list` | anunciado | anunciado | implementado no source atual; `delete` ACP não comprovado |
| Observer NIP-AO | sim, genérico sobre ACP | sim, genérico sobre ACP | esperado; validar instalado |
| Pronto para produção aqui | Juri funcional; faltam retomada durável e UI/config remota | falta runtime/auth isolados e prova E2E | falta instalar e executar conformidade |

O observer, a fila, o filtro de autores, as métricas e a composição de contexto
são do `buzz-acp`; não devem ser reimplementados por harness. Modelo, effort,
modos, permissions, retomada e conteúdo dos eventos de tools dependem do
adapter. O teste deve sempre partir das capacidades retornadas, sem ramificar
por marca quando o ACP já fornece uma categoria estável.

### 4.2 Transporte de prompt

O Buzz compõe uma única instrução permanente nesta ordem:

```text
base Buzz + workspace + persona + team instructions + memória + canvas
```

O transporte muda por adapter:

- **Claude:** o Buzz reconhece o nome oficial do adapter e envia a instrução no
  `session/new` pela extensão `_meta.systemPrompt: {"append": ...}`. O adapter
  preserva o preset nativo de tool use. O CLI atual grava snapshot desse prompt
  no histórico; uma mudança de persona deve invalidar o binding antigo ou abrir
  uma nova sessão, em vez de alterar silenciosamente o papel de uma conversa.
- **Codex:** a versão instalada negocia ACP v1 e não anuncia transporte próprio
  de system prompt. O Buzz entrega a instrução na primeira mensagem de usuário
  da sessão. `AGENTS.md` e skills do `CODEX_HOME` continuam sendo mecanismos
  nativos do Codex para políticas estáveis do workspace. No clone jurídico, o
  `AGENTS.md` ainda descreve a arquitetura Deep/Pleroma e possui maior autoridade
  operacional que o bloco de usuário; precisa ser atualizado antes de validar a
  fidelidade da persona.
- **OpenCode:** a documentação atual também declara ACP v1; portanto o Buzz usa
  o mesmo fallback da primeira mensagem. O próprio OpenCode carrega
  `AGENTS.md`, instructions, agent prompt, skills, plugins e config do diretório
  antes de criar a sessão.

Isso não impede os três harnesses, mas importa para segurança: política que
precisa de precedência forte não deve existir somente no texto da primeira
mensagem. Ela deve residir também no mecanismo nativo do harness e, para acesso
a dados, na autorização determinística das tools/BFF.

### 4.3 Modelos, effort e modos

As respostas reais de `session/new` confirmaram:

- **Codex:** modelos Astra, Sol, Terra, Luna e 5.5; o catálogo efetivo combina
  modelo e effort. `reasoning_effort` anuncia `low`, `medium`, `high`, `xhigh`,
  `max` e, quando suportado pelo modelo, `ultra`. O probe sem override iniciou
  em `agent`; o rollout do Juri confirma `gpt-5.6-luna`, effort `max` e modo
  `agent-full-access` após a configuração do serviço.
- **Claude:** `default`, Opus, Fable, Sonnet e Haiku no catálogo desta conta;
  effort `default`, `low`, `medium`, `high`, `xhigh`, `max`. Esses nomes são
  dinâmicos e devem vir do adapter, nunca de uma lista fixa mantida por nós.
- **OpenCode:** modelo é `provider/model`; effort é a variante do modelo; mode
  escolhe um agente primário visível. A permissão efetiva continua na config do
  OpenCode por tool/padrão.

Há duas incompatibilidades de semântica que exigem configuração, não fork:

1. `BUZZ_ACP_PERMISSION_MODE` usa o vocabulário do Claude. O Codex anuncia
   `agent-full-access`, que não existe no enum atual do `buzz-acp`; nosso Juri
   usa corretamente o env nativo `INITIAL_AGENT_MODE=agent-full-access`.
2. No OpenCode, `mode` identifica um agente, não um preset genérico de
   aprovação. O `buzz-acp` só tenta aplicar um valor anunciado e, portanto,
   ignora com segurança os valores incompatíveis. As permissões devem ser
   declaradas no agente OpenCode.

O cliente ACP do Buzz responde sozinho aos pedidos de permissão com
`allow_once` quando essa opção existe. Para Codex e Claude em modo autônomo isso
é uma decisão consciente do runtime. Para OpenCode, uma regra `ask` também seria
convertida em aprovação automática. Até existir aprovação interativa owner-side,
agentes OpenCode de produção devem usar regras determinísticas `allow`/`deny` e
o isolamento do systemd; não se deve interpretar `ask` como uma barreira humana.

### 4.4 MCP, tools e observer

`buzz-acp` passa servidores MCP em `session/new`. O builtin Codex configura
`buzz-dev-mcp` por padrão; Claude e OpenCode não recebem MCP automático do
catálogo, embora aceitem servidores entregues pelo cliente e seus próprios
arquivos de configuração. Um futuro servidor de tools da solução deve entrar
nesse ponto comum e ser testado nos três adapters.

OpenCode oferece ainda plugins capazes de registrar tools e namespaces ao lado
do harness. Eles são uma extensão nativa válida para capacidades exclusivas do
OpenCode. Uma capacidade de produto que precisa operar igual em Codex, Claude e
OpenCode deve usar um contrato portável, como MCP/HTTP assinado ou uma tool
equivalente em cada runtime, e não depender somente de plugin OpenCode.

O observer não depende de Codex. Ele copia o wire ACP e os `session/update` para
NIP-AO. Assim, qualquer adapter que transmita texto, reasoning, tool calls,
permissões e uso pode aparecer no painel Activity. Codex e Claude instalados
anunciam steering e emitem essas atualizações; OpenCode documenta streaming de
texto, reasoning, tools, permissions e usage, mas ainda precisa de prova local.

Há duas limitações genéricas adicionais na ponte atual:

- OpenCode emite `usage_update`; o tracker agora reconhece o adapter por
  `agentInfo`/capability e publica o custo acumulado quando o formato o prova.
- os três adapters anunciam imagem/embedded context, mas `buzz-acp` constrói os
  prompts somente com blocos de texto. Anexos do Buzz ainda não atravessam a
  ponte ACP como conteúdo multimodal.

### 4.5 Sessões

Os dois adapters instalados retornaram `loadSession: true` e anunciaram
`resume`, `fork`, `list`, `close` e `delete`. O source atual do OpenCode comprova
`load`, `resume`, `fork`, `list` e `close`; a documentação cita delete, mas a
classe ACP auditada não o expõe, então delete permanece não comprovado.
Essa capacidade continua inutilizada pelo Buzz porque `AcpClient` implementa
`session/new`, prompt, cancel, model e config, mas ainda não implementa
`session/load`/`resume`. O acoplamento de persistência deve ser genérico ACP e
não codificado para thread IDs do Codex.

## 5. Identidade, RBAC e autorização

O Buzz possui três controles diferentes. Eles não são substitutos entre si.

### 5.1 Relay e canal

O relay autentica a chave do ator e aplica comunidade, membership e papel do
canal. Isso decide se a pessoa pode ler, publicar e administrar o espaço.

Os três significados de owner são distintos:

```text
community owner ≠ channel owner ≠ agent owner por NIP-OA
```

Nenhum deles implica automaticamente papel jurídico, acesso a case, orçamento
ou capability de tool. O enum de canal ainda descreve `guest` como read-only,
mas o caminho genérico de ingest valida membresia/canal aberto sem consultar
esse papel. Não devemos usar `guest` como barreira de escrita até confirmar ou
corrigir o comportamento upstream.

### 5.2 Propriedade do agente

NIP-OA liga o agente ao owner. Essa relação permite ao Desktop reconhecer
`managed by you`, receber telemetry criptografada e enviar controles de owner.

### 5.3 Admissão de mensagens pelo harness

`buzz-acp` possui os modos:

```text
owner-only
allowlist
anyone
nobody
```

O Juri está em `owner-only`. Hoje somente o owner e identidades irmãs admitidas
pela regra nativa conseguem iniciar turnos. Colocar dois advogados no canal não
autoriza automaticamente os dois a usar o Juri.

Em DM, a implementação é ainda mais restritiva: mesmo `anyone` e `allowlist`
admitem somente owner ou agentes irmãos. Um prospect comum não ativa hoje um
agente pelo fluxo DM nativo do `buzz-acp`.

O prompt entregue ao harness contém o autor verificado:

```text
From: Nome (npub: ..., hex: ...)
Event ID: ...
Channel: ...
```

Assim o modelo sabe com quem conversa. Isso não cria autorização forte para um
BFF. O MCP entregue ao harness recebe `BUZZ_PRIVATE_KEY`, `BUZZ_RELAY_URL` e
`BUZZ_AUTH_TAG` do agente. Ele não recebe, por um canal confiável separado do
texto, a identidade do humano que originou aquele turno.

Consequência: uma tool não deve autorizar acesso a caso, métrica ou dado
sigiloso apenas porque o modelo enviou `actor_pubkey` como argumento. Para o
produto multiusuário precisaremos de uma credencial por turno ou de um contexto
assinado que vincule:

```text
community + channel + event_id + requester_pubkey + agent_pubkey + expiry
```

Esse é um acoplamento legítimo caso o upstream não introduza o contexto de
principal nas chamadas ACP/MCP. A decisão de formato só deve ocorrer depois do
teste de tools com Codex, Claude e OpenCode.

NIP-AM também não carrega o pubkey do humano que originou o turno. Ele permite
FinOps por agente, canal, sessão e modelo; rateio e orçamento por
owner/advogado/prospect precisam correlacionar o evento de entrada no registro
de eval/policy.

### 5.4 Conceitos que não podem ser misturados

| Conceito | O que controla |
|---|---|
| Role de comunidade/canal | membership e governança no relay |
| NIP-OA owner | propriedade e controle do agente |
| Persona | comportamento, runtime e configuração do agente |
| Team | conjunto de personas e instruções compartilhadas |
| `respond_to` | quem pode iniciar um turno |
| Session policy | quem compartilha contexto ACP |
| Capability/tool | operação disponível ao harness |
| Grant do BFF | operação e recurso permitidos ao principal real |

## 6. Sessões e memória

### 6.1 Escopos nativos

Em `channel`, todas as mensagens admitidas no canal reutilizam a mesma sessão
ACP. Isso explica os testes anteriores que caíram na mesma thread Codex e dá ao
agente continuidade compartilhada.

Em `thread`, cada root NIP-10 possui sua própria sessão. Menções repetidas no
mesmo tópico a reutilizam. DMs continuam por conversa.

Isso não significa memória conversacional global. Sessões diferentes
compartilham core NIP-AE, workspace, acesso ao relay e autorização, mas não
compartilham automaticamente transcript, reasoning ou trabalho em andamento.
Quando uma sessão nova nasce, o `buzz-acp` reidrata uma janela limitada de
contexto — 12 mensagens por padrão, no máximo 100 — nos fluxos automáticos de
thread/DM. Uma menção top-level de canal não recebe o histórico completo. O
restante continua durável no relay e pode ser consultado pelo CLI.

```text
Sessão ACP
└── contexto interno do harness; perde o binding no restart do buzz-acp

Histórico Buzz
└── eventos persistentes no relay; consulta e reidratação limitada

NIP-AE
├── core: injetado em cada sessão nova
└── mem/*: memória fria consultada sob demanda
```

Uso recomendado:

| Cenário | Política inicial |
|---|---|
| Sala interna do escritório | `channel`, se o contexto for deliberadamente compartilhado |
| Caso jurídico separado | canal dedicado ou `thread` |
| Atendimento público em canal compartilhado | `thread` |
| Atendimento individual | DM |
| Agente de prospecção com relatório ao owner | DMs para prospects + canal interno para operação |

### 6.2 Restart

O estado atual de `buzz-acp` é:

```rust
SessionScope -> session_id
```

armazenado em um `HashMap` do processo. Após restart do relay, a sessão continua
se o processo do agente não reiniciar. Após restart do agente, o mapa desaparece
e `buzz-acp` executa `session/new`.

Nas versões auditadas, Codex e Claude anunciam e implementam `session/load`; o
source atual do OpenCode também implementa load/resume. Essa é capacidade dos
adapters, não do Buzz. O cliente ACP do Buzz não persiste o binding nem chama
load no startup. Antes de implementar armazenamento, precisamos fixar as regras
de invalidação (`!rotate`, troca de modelo, mudança de persona/prompt, membership
epoch e remoção do canal).

## 7. Observer e experiência na UI

O caminho nativo é:

```text
harness → ACP session/update → buzz-acp → NIP-AO → Buzz Desktop
```

O Desktop possui:

- sinal de agente trabalhando junto ao compositor;
- `View activity`;
- painel `Activity`;
- painel `Raw ACP activity`;
- tool calls, shell, arquivos, diffs, mensagens, erros e lifecycle;
- cancelamento do turno.

O archive local de observer é default-on no Desktop. O relay não persiste
`kind:24200`, pois ele é efêmero. O Desktop pode salvar os frames recebidos em
SQLite local e aplicar retenção. Como não existe backfill no relay, esse archive
só captura frames enquanto algum Desktop autenticado estiver conectado e
assinado ao observer. O Mobile possui um feed de Activity ao vivo mais simples;
não foi encontrado archive histórico equivalente ao Desktop.

Na primeira instalação, os frames eram emitidos mas rejeitados. O Juri possuía
simultaneamente `BUZZ_AUTH_TAG` NIP-OA e membership direto no relay. Em relay
fechado, o caminho de membro direto encerrava a decisão antes de materializar o
owner; `users.agent_owner_pubkey` ficava NULL e NIP-AO/NIP-AM owner-scoped eram
recusados. A correção operacional removeu somente o Juri do roster direto,
manteve o owner humano e reiniciou o agente. O relay também recebeu uma
correção upstream para materializar uma credencial NIP-OA válida mesmo quando o
agente já é membro direto. Depois disso:

```text
users.agent_owner_pubkey = 930907...
kind 24200 received: 164 → 171
WS auth rejects:      110 → 110
```

O relay está agora validado. O Desktop oficial 0.5.23 é anterior ao suporte
global para ingestão de observer de agentes remotos owned presente no source
auditado; o cliente precisa ser atualizado/construído desse source para fechar o
E2E visual.

### Limite de visibilidade

Os frames atuais são cifrados para o owner NIP-OA. Para o Juri atual, o owner
deve conseguir acompanhar a execução. Um advogado ou prospect que não seja
owner não recebe o transcript técnico detalhado. Há trabalho upstream aberto
para visibilidade por requester; não devemos criar um protocolo paralelo antes
de acompanhar essa evolução.

## 8. Modelo e effort na UI

O Buzz já possui configuração persistente de modelo e effort nos formulários de
agente/persona local. Também existem o frame `switch_model`, handlers no
`buzz-acp`, resultado assíncrono e o componente React `ModelPicker`.

O estado auditado é parcial:

- `ModelPicker.tsx` não possui consumidor ativo em `desktop/src`;
- o picker de effort é explicitamente `local-only`, persiste o valor e o aplica
  no começo da próxima sessão; não troca effort no meio do turno;
- os builtins Codex e Claude estão marcados
  `supports_acp_model_switching: false` no catálogo, embora seus adapters
  instalados anunciem e aceitem a opção estável `model`;
- a troca ao vivo do backend funciona somente quando a UI consegue nomear um
  canal com turno ativo; o valor é runtime-only e se perde no respawn;
- descoberta e escrita para agentes em host remoto ainda passam pelo registro
  de managed agents e assumem binário/backend local em partes do Desktop;
- há issue upstream específica para agentes de host persistente e model picker;
- o controle por UI do Juri não deve ser considerado validado até aparecer e
  trocar modelo em um turno real.

O backend nativo deve ser preservado; o acoplamento necessário é expor esse
contrato a agentes remotos persistentes e montar o controle existente na UI ou
no wrapper. Não há justificativa para um roteador automático de modelos.

## 9. Auditoria da implantação atual

### 9.1 Partes alinhadas

- relay oficial em container;
- Postgres e Redis externos existentes;
- MinIO dedicado conforme o padrão do Buzz;
- bind somente em `10.20.0.1:3300`;
- agente no host, sob usuário Linux dedicado;
- `HOME` e `CODEX_HOME` isolados;
- runtime ACP root-owned;
- NIP-OA, perfil, runtime directory e política gerenciada publicados;
- workspace jurídico separado em `/srv/buzz-agents/juri-root`;
- observer habilitado.

### 9.2 Divergências encontradas

| Divergência | Efeito | Estado |
|---|---|---|
| observer ausente no unit manual | UI não via atividade ACP | corrigido |
| agente NIP-OA também estava no roster direto | relay não materializava owner e rejeitava NIP-AO/NIP-AM | corrigido; provisionador atualizado |
| `AF_NETLINK` ausente | primeiro comando `bwrap` falhava | corrigido |
| command apontava para `dist/index.js` | Buzz não reconhecia `codex-acp` | corrigido com bin canônico |
| `bypassPermissions` não é modo Codex | modo `agent` executava auto-review e bloqueava a publicação inicial | workaround funcional `INITIAL_AGENT_MODE=agent-full-access`; não serve de política para agente público |
| repo jurídico fixava Sol High | consultas simples usavam configuração cara | runtime agora projeta Luna Max; UI ainda deve ser validada |
| `AGENTS.md` contém caminhos do host antigo | decisões e comandos inválidos no VPS | corrigido e commitado no repo jurídico |
| `./venv/bin/python` não existe no clone | comandos do contrato falham | `.venv` criado, `venv` legado compatível e suíte validada |
| `agent.env` e `runtime.env` continham command/mode divergentes | duas fontes de verdade | corrigido: segredos em `agent.env`; política runtime em `runtime.env` |
| base prompt frio de 17,8 KB + help obrigatório | contexto e ciclo de inferência excessivos em conversa simples | usar `BUZZ_ACP_BASE_PROMPT_FILE` compacto |
| Docker `json-file` sem rotação | crescimento ilimitado | corrigido: `50m × 5` |
| binding de sessão volátil | nova sessão após restart do serviço | corrigido no `buzz-acp` com SQLite WAL e `session/load` |

`INITIAL_AGENT_MODE=agent-full-access` é configuração oficial do `codex-acp`.
Ela remove o segundo ciclo de aprovação dentro do Codex. O processo continua
limitado pelo usuário `buzz-juri-root`, `ProtectHome`, `ProtectSystem=strict`,
`ReadWritePaths` e capability set vazio do systemd.

Isso é uma decisão explícita de segurança × latência, não um default universal.
O modo `agent` criou threads ocultas de auto-review e multiplicou inferência; o
modo `agent-full-access` eliminou esse custo, mas concede ao harness acesso de
rede e leitura/escrita dentro da fronteira permitida ao usuário do serviço. É
aceitável apenas no Juri `owner-only` de validação. Um agente público deve usar
broker keyless, tools fechadas e policies determinísticas antes de receber
autoridade equivalente.

## 10. Latência observada

O relay não é o gargalo atual:

```text
HTTP relay:       mediana ~24 ms; faixa observada 19–54 ms
host load:        variável; snapshot não é propriedade do deployment
relay container:  ~0,1% CPU
MinIO container:  ~0,1% CPU
```

Turnos medidos:

| Turno | Modelo | Duração Codex | O que ocorreu |
|---|---|---:|---|
| `ta on?` | Sol High | 32,0 s | primeira sessão, CLI e publicação |
| `quais os ramos...` | Sol High | 173,8 s | skill jurídica, hashes, catálogo, 12 execuções e 2 falhas |
| `PERSISTENCIA-OK` | Luna Max | ~4 s | sessão criada após o primeiro restart com store durável |
| `RESTORE-OK` | Luna Max | ~8 s | mesmo `session_id` restaurado após novo restart |
| `ACTIVITY-OK` | Luna Max | ~4 s | NIP-AO aceito; rejeições auth permaneceram estáveis |
| teste observer 1 | Luna Max | 25,1 s | publicação falhou por sandbox e foi repetida |
| teste observer 2 | Luna Max | 39,5 s | mesmo ciclo de falha/repetição; modo Codex ainda era `agent` |
| turno limpo | Luna Max | 15,5 s | sem falha/retry; primeira sessão ainda consultou `messages send --help` |
| prompt compacto | Luna Max | 16,4 s total; ~7 s até publish | 1 comando, 0 falhas, sem help; contexto caiu para ~19,8 mil tokens |

A consulta sobre os ramos acionou corretamente `corpus-research`, mas executou
uma verificação de integridade completa para responder metadado estável. A
otimização correta não é enfraquecer a regra jurídica. É materializar um índice
de capacidades verificado no deploy e fazer a skill usar esse índice para
perguntas de catálogo, reservando hashes por objeto para pesquisa jurídica real.

O primeiro turno limpo após `agent-full-access` publicou a resposta em cerca de
14 segundos e concluiu em 15,5 segundos. O log confirmou
`approval_policy=Never` e `sandbox_policy=DangerFullAccess`. A primeira sessão
ainda pagou uma consulta de help do CLI. Os próximos evals devem registrar
separadamente:

```text
relay ingress
session resolve/new/load
primeiro evento ACP
inferência até primeira saída
tempo de tools
publicação Buzz
turn completed
```

Correção da contagem: o turno jurídico longo registrou **12** itens
`commandExecution`, com duas falhas. O custo dominante não foi a duração dos
processos, mas os ciclos de inferência e, no modo Codex `agent`, as revisões
automáticas. A thread principal acumulou aproximadamente 545 mil tokens de
processamento e a auto-review cerca de 239 mil naquele turno. Nos testes Luna
anteriores ao full access, também surgiram threads `codex-auto-review`.

Há ainda um custo fixo alto por sessão fria: o `base_prompt.md` atual tem cerca
de 17,8 KB e a primeira mensagem do teste mínimo chegou a aproximadamente 18,6
mil caracteres. O turno limpo registrou cerca de 67,9 mil tokens ao somar seus
ciclos de inferência. O Buzz oferece `BUZZ_ACP_BASE_PROMPT_FILE`; um contrato
compacto específico do produto, mantendo framing, identidade, threading,
publicação e memória, é a otimização nativa adequada. Desligar o base prompt
inteiro não é aceitável.

O teste com `BUZZ_ACP_BASE_PROMPT_FILE` reduziu o base de 17.771 para 3.568
bytes e o processamento acumulado do turno simples de cerca de 67,9 mil para
19,8 mil tokens. A resposta foi publicada aproximadamente 7 segundos após o
início do Codex; o turno só terminou em 16,4 segundos porque o harness fez um
segundo ciclo pós-tool para produzir o final ACP interno. Para UX do canal, a
latência relevante é o publish; para custo e capacidade, o segundo ciclo também
conta. Luna Max continua sendo deliberadamente mais lento que efforts menores.

## 11. Logs e telemetria nativos

| Fonte | Conteúdo | Persistência |
|---|---|---|
| NIP-AO | transcript ACP e lifecycle | efêmero no relay; archive local do Desktop |
| NIP-AM | tokens, cache, modelo e custo | evento persistente e archive local |
| journald | `buzz-acp`, adapter e stderr | host |
| Codex SQLite/JSONL | threads, itens, turnos e logs internos | HOME isolado |
| Docker relay/MinIO | logs dos containers | `json-file` local |
| `buzz-audit` | operações de domínio do relay | Postgres/hash chain |

NIP-AM não mede toda a decomposição de latência. NIP-AO permite reconstruir
parte do lifecycle enquanto o Desktop está conectado/arquivando. O coletor de
eval da migração deve consumir primeiro esses dois contratos, acrescentando
somente as fases ausentes. Para OpenCode, o `usage_update` já chega ao observer,
mas o tracker NIP-AM ainda precisa deixar de reconhecer adapters por whitelist
Codex/Claude.

Na implantação atual:

- relay e MinIO usam `json-file` com `max-size=50m` e `max-file=5`;
- journald já está limitado globalmente a aproximadamente 256 MB e 7 dias;
- o `CODEX_HOME` do Juri ocupa cerca de 66 MB, majoritariamente cache/logs;
- o relay expõe Prometheus em `:9102`, coletado pelo Prometheus em `10.20.0.1:3310`;
- OTLP está ativo para Tempo, com traces do serviço `buzz-relay` indexados;
- Grafana está disponível em `10.20.0.1:3311`, com Prometheus, Loki e Tempo;
- Loki recebe journald dos units Buzz e do bridge Docker, filtrando serviços da Pleroma;
- o Juri possui cinco eventos `kind:44200` persistidos, todos com `#p` do owner;
- NIP-AM/FinOps nativo do Juri está validado no caminho Codex; rateio por requester
  ainda exige correlação externa porque o evento não identifica o humano;
- três restarts registraram `Failed to kill control group ... Invalid argument`,
  sem processo órfão observado. A causa precisa ser isolada.

## 12. Pontos de extensão corretos

### 12.1 Roles, skills e toolkits

O desenho desejado já aparece no `PERSONA_PACK_SPEC`:

```text
pack
├── tools/MCP compartilhados
├── skills compartilhadas
└── personas
    ├── role/system prompt
    ├── skills específicas
    └── MCP servers específicos
```

O schema e o resolver de `buzz-persona` aceitam `skills` e `mcp_servers` por
persona. O merge previsto usa o pack como base e a persona como override. Esse
é o lugar conceitual correto para designar conjuntos de capacidades a advogado,
juiz, promotor, prospector ou supervisor.

O estado implementado ainda é parcial:

- parser, validação e merge existem em `buzz-persona`;
- `buzz pack validate` e `buzz pack inspect` existem;
- cópia de skills para o workspace está marcada como planejada;
- pack e snapshot/agent definition do Desktop ainda não são conversíveis;
- o `buzz-acp` atual cria somente o servidor indicado por
  `BUZZ_ACP_MCP_COMMAND`; não há wiring operacional do conjunto resolvido da
  persona encontrado no caminho de runtime;
- os catálogos compartilháveis de team omitem env vars e allowlists por
  segurança.
- hooks são parseados/especificados, mas a execução runtime end-to-end também
  permanece incompleta;
- interpolação de env MCP descrita no spec ainda está marcada como planejada.

Logo, não criaremos um segundo formato de role/toolkit. A definição inicial
deve acompanhar o Persona Pack Spec e separar quatro conceitos:

```text
role/persona       comportamento e função
skill              instrução/procedimento carregável
capability/tool    operação executável
grant              autorização de principal/contexto para executar
```

Associar uma tool à persona diz que ela está disponível ao harness. Isso não
autoriza qualquer usuário a executá-la nem qualquer linha de dados a ser lida.
Os grants precisam permanecer verificáveis no boundary da tool/BFF.

O documento upstream
[`docs/practical-information-flow-for-buzz-agents.md`](../../docs/practical-information-flow-for-buzz-agents.md)
já propõe a base de segurança apropriada: um trusted broker mantém a chave do
agente, deriva requester/audiência/epoch do evento verificado, executa instâncias
keyless ligadas a uma audiência e expõe somente ações semânticas fechadas. Ele
também declara que integridade e autorização de tools ficam para uma extensão
posterior, citando um caminho FIDES-style.

O repositório já possui duas fundações para esse caminho: `ifc-core`, com labels
de confidencialidade por reader set, e o contrato estrito `buzz-sdk::broker`
para agentes sem chave e ações semânticas. O host, transporte, signing e grants
desse broker ainda não estão conectados ao `buzz-acp` usado pelo Juri.

Nossa definição de capability grant deve evoluir esse contrato, e não colocar
RBAC em texto de prompt:

```text
verified event
  → execution domain (agent, audience, context, epoch)
  → persona capability set
  → principal/context grant
  → semantic tool adapter
  → BFF/domain authorization
  → audited result with information label
```

Enquanto esse broker não está implementado no caminho usado, agentes públicos
com tools ou dados privados não devem ser tratados como isolados apenas por
sessão. O processo atual ainda compartilha credenciais, arquivos, caches e
autoridade ambiental entre sessões. `owner-only` limita o Juri de validação,
mas não resolve o produto público futuro.

### Usar sem alteração

- relay e tipos de evento;
- canais, comunidades e memberships;
- NIP-OA, NIP-AE, NIP-AO e NIP-AM;
- personas, team instructions e runtime catalog;
- schema de persona packs para skills e MCP, respeitando o estado parcial;
- `buzz-acp` para fila, prompt, sessões e observer;
- presets ACP de Codex, Claude e OpenCode;
- workflows e webhooks do Buzz quando forem adequados.

### Extensões nativas antes de fork

- `buzz-backend-*` para substrato remoto;
- custom harness definition para um ACP não catalogado;
- persona packs para templates de agentes;
- o sidecar MCP stdio atual do `buzz-acp`; HTTP MCP apenas quando suportado e
  configurado pelo adapter/harness;
- eventos Nostr para novas capacidades de produto.

### Lacunas candidatas a acoplamento

1. binding durável de `SessionScope -> ACP session_id`;
2. principal humano confiável nas tools/BFF;
3. observabilidade persistente central e evals de migração;
4. provider para host persistente/systemd, se o upstream não o entregar;
5. branding e composição do wrapper cliente, mantendo o core atualizado.
6. wiring de capabilities por persona, preferencialmente contribuindo para o
   contrato de Persona Packs em vez de criar outro manifesto.

Nenhuma dessas lacunas deve ser resolvida dentro do BFF jurídico ou com código
específico do Juri. O contrato precisa funcionar para Codex, Claude Code e
OpenCode.

## 13. Plano de validação

### Fase A — conformidade do que já existe

1. validar Activity e Raw ACP Activity do Juri no Desktop do owner;
2. medir um turno limpo com o modo Codex corrigido;
3. corrigir caminhos/venv do clone jurídico sem alterar o contrato do Buzz;
4. validar model/effort persistente na UI existente;
5. confirmar o comportamento do archive local e NIP-AM;
6. colocar rotação nos logs Docker.

### Fase B — matriz de harnesses

Para Codex, Claude e OpenCode executar o mesmo conjunto:

1. initialize e capacidades anunciadas;
2. session/new;
3. dois turnos na mesma sessão;
4. tools e publicação Buzz;
5. observer e uso;
6. modelo/effort/mode;
7. restart do adapter;
8. session/load/resume;
9. erro de autenticação, timeout e cancelamento;
10. isolamento de HOME/workspace.

### Fase C — segurança multiusuário

1. owner;
2. advogado allowlisted;
3. membro comum;
4. usuário de outra comunidade;
5. prospect em DM;
6. tentativa de consultar dado de outro principal;
7. tentativa de invocar tool sem capacidade por turno.

Somente após essas três fases deve ser fechado o desenho dos acoplamentos.

## 14. Fontes principais

### 14.1 Evidência local e pontos de código

| Conclusão | Fonte |
|---|---|
| Codex e Claude são builtins; skill dirs, MCP default e flags de config | [`catalog.rs:50`](../../desktop/src-tauri/src/managed_agents/discovery/catalog.rs#L50) |
| OpenCode é preset `opencode acp` | [`presets.rs:167`](../../desktop/src-tauri/src/managed_agents/discovery/presets.rs#L167) |
| Desktop injeta prompt, model, effort e observer no processo local | [`runtime.rs:633`](../../desktop/src-tauri/src/managed_agents/runtime.rs#L633) |
| Binding de sessões é `HashMap` em memória | [`pool.rs:126`](../../crates/buzz-acp/src/pool.rs#L126) |
| Seleção do transporte de system prompt | [`pool.rs:309`](../../crates/buzz-acp/src/pool.rs#L309) |
| Criação da sessão, model/effort/mode e captura para a UI | [`pool.rs:1490`](../../crates/buzz-acp/src/pool.rs#L1490) |
| Fallback de contexto permanente para o primeiro user prompt | [`pool.rs:2073`](../../crates/buzz-acp/src/pool.rs#L2073) |
| Cliente ACP tem `session/new`, model e config, sem load/resume | [`acp.rs:644`](../../crates/buzz-acp/src/acp.rs#L644) |
| Permission requests são autoaprovadas com `allow_once` | [`acp.rs:1944`](../../crates/buzz-acp/src/acp.rs#L1944) |
| Usage standard é classificado somente como Codex/Claude | [`acp.rs:541`](../../crates/buzz-acp/src/acp.rs#L541) |
| Model picker implementado, porém sem montagem na árvore React | [`ModelPicker.tsx:28`](../../desktop/src/features/agents/ui/ModelPicker.tsx#L28) |
| Effort picker é local e vale para a próxima sessão | [`EffortPickerField.tsx:9`](../../desktop/src/features/agents/ui/EffortPickerField.tsx#L9) |
| Codex anuncia load/resume, modelos e transportes MCP | `/opt/buzz-agent-runtime/node_modules/@agentclientprotocol/codex-acp/dist/index.js:32325` |
| Codex implementa resume/load sobre `threadResume` | `/opt/buzz-agent-runtime/node_modules/@agentclientprotocol/codex-acp/dist/index.js:28530` |
| Codex expõe model e `thought_level` | `/opt/buzz-agent-runtime/node_modules/@agentclientprotocol/codex-acp/dist/index.js:30043` |
| Claude anuncia load/resume e MCP | `/home/ubuntu/.local/lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js:920` |
| Claude implementa resume/load e replay | `/home/ubuntu/.local/lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js:993` |
| Claude preserva preset e anexa system prompt | `/home/ubuntu/.local/lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/acp-agent.js:5909` |

Os caminhos `/opt` e `/home/ubuntu/.local` registram a evidência desta máquina;
não são dependências portáveis do documento. Os adapters devem permanecer
pinados por versão no runtime comum e ser interrogados novamente após upgrade.

### 14.2 Especificações e documentação

- [`ARCHITECTURE.md`](../../ARCHITECTURE.md)
- [`docs/remote-agents.md`](../../docs/remote-agents.md)
- [`docs/nips/NIP-AO.md`](../../docs/nips/NIP-AO.md)
- [`docs/nips/NIP-AM.md`](../../docs/nips/NIP-AM.md)
- [`crates/buzz-acp/src/scope.rs`](../../crates/buzz-acp/src/scope.rs)
- [`crates/buzz-acp/src/pool.rs`](../../crates/buzz-acp/src/pool.rs)
- [`crates/buzz-acp/src/base_prompt.md`](../../crates/buzz-acp/src/base_prompt.md)
- [`desktop/src-tauri/src/managed_agents/runtime.rs`](../../desktop/src-tauri/src/managed_agents/runtime.rs)
- [`desktop/src-tauri/src/managed_agents/discovery/catalog.rs`](../../desktop/src-tauri/src/managed_agents/discovery/catalog.rs)
- [`desktop/src-tauri/src/managed_agents/discovery/presets.rs`](../../desktop/src-tauri/src/managed_agents/discovery/presets.rs)
- [OpenCode ACP](https://opencode.ai/v2/docs/cli/acp/)
- [OpenCode Agents](https://opencode.ai/v2/docs/agents)
- [OpenCode Models](https://opencode.ai/v2/docs/models)
- [OpenCode Permissions](https://opencode.ai/v2/docs/permissions)
- [OpenCode plugins](https://opencode.ai/v2/docs/build/plugins)
- [OpenCode ACP source auditado](https://github.com/anomalyco/opencode/tree/ae93d4afb3e414a541f520e062be924573b126e1/packages/opencode/src/acp)
- [Codex ACP](https://github.com/agentclientprotocol/codex-acp)
- [Claude Agent ACP](https://github.com/agentclientprotocol/claude-agent-acp)
- [Buzz issue: requester observer visibility](https://github.com/block/buzz/issues/2716)
- [Buzz issue: persistent remote hosts/model picker](https://github.com/block/buzz/issues/5282)

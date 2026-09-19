# Plano de remediação Buzz no VPS

Data-base: 19 de setembro de 2026
Fonte: [`BUZZ_NATIVE_AUDIT.md`](./BUZZ_NATIVE_AUDIT.md)

## Princípios

1. Usar primeiro a capacidade nativa do Buzz ou do adapter ACP.
2. Corrigir upstream quando o contrato já existe, mas o wiring está incompleto.
3. Criar acoplamento externo somente para lacuna confirmada.
4. Todo acoplamento precisa funcionar com Codex, Claude Code e OpenCode.
5. Prompt e skill orientam comportamento; autorização permanece no broker/BFF.
6. Nenhum agente público com dados privados entra em produção antes do broker.

## Estado já corrigido

- `AF_NETLINK` permitido no serviço do Juri;
- comando canônico `codex-acp` usado no runtime;
- observer NIP-AO habilitado;
- Luna Max e base prompt compacto configurados;
- `agent.env` separado de `runtime.env`;
- Juri removido do roster direto e autenticado por NIP-OA;
- `users.agent_owner_pubkey` materializado corretamente;
- frames NIP-AO aceitos pelo relay sem aumento de rejeições auth;
- primeiro NIP-AM `kind:44200` do Juri persistido;
- `AGENTS.md` do clone adaptado ao Buzz;
- contrato pré-Buzz preservado em backup restrito;
- provisionador deixou de recomendar membro direto para agente NIP-OA.

## P0 — fechar a validação atual

### P0.1 Activity no Desktop

**Tipo:** usar source upstream atual / atualização do cliente.

O servidor já foi comprovado:

```text
kind 24200 received: 164 → 171
WS auth rejects:      110 → 110
```

A release oficial Desktop 0.5.23 é anterior ao suporte global para observer de
agentes remotos `declared-owned`, incorporado no source em 16/09/2026.

Entregas:

1. confirmar a versão instalada no Windows;
2. construir/instalar Desktop a partir de commit que contenha
   `useAgentObserverIngestion` para relay-owned agents;
3. abrir Activity no mesmo canal do turno;
4. validar Activity, Raw ACP, tool call, output e `turn_completed`;
5. validar archive local após fechar/reabrir o painel.

Aceite:

- probe em `#geral` aparece ao vivo;
- painel do DM mostra somente eventos do DM;
- nenhum frame de outro canal aparece no escopo selecionado;
- reconnect não duplica eventos.

### P0.2 Correções upstream de NIP-OA/observer

**Tipo:** patch upstream Buzz.

1. Em relay fechado, validar/materializar NIP-OA mesmo quando o agente também é
   membro direto.
2. Adicionar teste:

```text
direct member + valid NIP-OA
→ owner materializado
→ observer aceito
```

3. Fazer `buzz-acp` registrar WARN e contador quando um frame `24200` recebe
   `OK accepted=false`; hoje a rejeição fica em DEBUG.
4. Manter o provisionamento local no caminho correto: humano owner no roster,
   agente somente por NIP-OA.

### P0.3 Juri jurídico

**Tipo:** configuração do produto jurídico.

1. manter o novo `AGENTS.md` e o backup pré-Buzz;
2. definir ambiente Python reproduzível a partir de `pyproject.toml`;
3. materializar `capabilities-index.json` no preflight do corpus;
4. responder perguntas de catálogo pelo índice verificado;
5. reservar verificação por objeto para pesquisa jurídica real;
6. medir novamente pergunta de hora, catálogo e consulta jurídica.

Aceite:

- nenhum comando usa `./venv/bin/python` inexistente;
- nenhuma instrução referencia host/path Deep antigo;
- pergunta de catálogo não refaz auditoria completa;
- pesquisa de conteúdo preserva hashes e revisão humana.

### P0.4 Logs Docker

**Tipo:** configuração operacional.

Adicionar a relay, MinIO e init:

```yaml
logging:
  driver: json-file
  options:
    max-size: "50m"
    max-file: "5"
```

Aplicar em janela curta de manutenção, pois os containers precisam ser
recriados. Não alterar o limite global de journald neste momento.

## P1 — matriz de harnesses e controles da UI

### P1.1 Runtime isolado

Empacotar versões pinadas no runtime root-owned:

```text
/opt/buzz-agent-runtime
├── codex-acp
├── claude-agent-acp
└── opencode
```

Cada agente recebe usuário Linux, HOME, auth e workspace próprios.

### P1.2 Conformidade comum

Executar em Codex, Claude Code e OpenCode:

1. initialize/capabilities;
2. session/new;
3. dois turnos na mesma sessão;
4. tools e publicação Buzz;
5. observer e NIP-AM;
6. model/effort/mode;
7. cancel/steer;
8. restart + load/resume;
9. erro de auth, timeout e limite;
10. isolamento de HOME/workspace.

### P1.3 ModelPicker e effort

**Tipo:** patch upstream/UI reaproveitável pelo wrapper.

1. montar o `ModelPicker` existente nos controles de Activity;
2. listar apenas opções anunciadas pelo adapter;
3. persistir default separado do override da sessão;
4. expor `thought_level` somente quando anunciado;
5. aplicar effort ao vivo via `session/set_config_option` quando suportado;
6. esconder controles sem capability;
7. restringir alterações ao agent owner.

Não manter catálogo fixo por fabricante e não criar roteamento automático.

## P2 — persistência genérica de sessões

**Tipo:** patch upstream no `buzz-acp`.

Persistir:

```text
agent pubkey
community/relay
SessionScope
ACP session_id
adapter identity/version
cwd
prompt/persona revision
model/effort/mode
membership epoch
timestamps
```

Startup:

1. adapter anuncia `loadSession`/resume;
2. binding é compatível com adapter, cwd e prompt revision;
3. `buzz-acp` chama `session/load`;
4. sucesso restaura counters/delivery state;
5. falha marcada como stale cria `session/new`;
6. nenhum fallback apaga a evidência anterior silenciosamente.

Invalidar em:

- `!rotate`/nova sessão;
- mudança de persona ou system prompt;
- troca incompatível de harness;
- remoção do canal;
- mudança de membership epoch;
- delete/archive explícito.

Aceite: restart de relay não reinicia o agente; restart de systemd retoma a
mesma sessão nos três harnesses quando compatível.

## P3 — multiusuário, RBAC e toolkits

### P3.1 Separação conceitual

```text
respond_to      → pode iniciar turno
persona/toolkit → operações visíveis ao harness
broker grant    → operações permitidas ao principal neste turno
BFF             → registros e campos realmente acessíveis
```

### P3.2 Trusted Agent Policy Broker

**Tipo:** acoplamento genérico alinhado a `buzz-sdk::broker` e `ifc-core`.

```text
evento Buzz verificado
  → execution domain(agent, audience, context, epoch)
  → persona capability set
  → grant curto e não forjável
  → adapter semântico de tool
  → BFF autoriza operação/recurso
  → resultado e decisão auditados
```

Contexto mínimo:

```text
community_id
agent_pubkey / agent_owner_pubkey
actor_pubkey
channel/thread/event_id
session_id / turn_id
community_role / channel_role
audience / membership_epoch
persona_id
capabilities
budget
expiry / nonce
```

Regras:

- modelo nunca declara o principal;
- tool nunca confia em `actor_pubkey` fornecido como argumento do modelo;
- capability é vinculada ao turno e protegida contra replay;
- BFF continua impondo autorização por tenant, case, row e field;
- skill ensina; não concede permissão;
- esconder tool melhora UX, mas não é controle de segurança;
- allow/deny registra principal, capability, recurso, evento e turno.

### P3.3 Usuários internos e públicos

Antes do broker:

- Juri permanece `owner-only`;
- advogados podem ser canariados com `allowlist`, canal/case dedicado e sem
  capability administrativa;
- não liberar agente público com BFF credenciado ou dados privados.

Depois do broker:

- advogados: capabilities por case atribuído;
- owner do cliente: métricas e operações tenant-wide;
- prospect: operações self-only;
- agentes irmãos: grants explícitos, nunca implícitos por NIP-OA.

### P3.4 DM público

O bloqueio atual é deliberado. Criar upstream uma política separada:

```text
BUZZ_ACP_DM_RESPOND_TO=owner-only  # default
BUZZ_ACP_DM_ALLOWLIST=...
```

`anyone` precisa ser opt-in, com membership, anti-spam, budget e rate limit.
Até isso existir, atendimento público usa canal dedicado + session policy
`thread` + processo/key/workspace sem dados internos.

## P4 — agentes systemd gerenciados

**Tipo:** provider compatível com `docs/remote-agents.md`.

Primeiro acompanhar/adotar o provider systemd/SSH upstream. Se ele não chegar,
implementar `buzz-backend-systemd`, mantendo o protocolo existente.

Requisitos:

- deploy idempotente;
- uma instância por pubkey/escopo;
- unit/drop-ins escritos atomicamente;
- secrets fora de `provider_config` e stdout;
- usuário, HOME e workspace isolados;
- Codex, Claude e OpenCode intercambiáveis;
- presença como status;
- start só conclui após conexão do harness;
- shutdown intencional não reinicia;
- reconcile de desired state;
- model/effort/prompt projetados pelo Desktop.

## P5 — observabilidade, FinOps e evals

```text
Prometheus ← relay :9102
OTel Collector/Alloy ← OTLP do relay
Loki/journald ← systemd + Docker
NIP-AO archive ← transcript técnico owner-side
NIP-AM ← tokens/custo/modelo
Eval store ← resultado funcional da migração
```

Entregas:

1. scraper privado para `:9102`;
2. dashboard de relay, WS, auth, DB, media e storage;
3. OTLP do relay apontado para collector interno;
4. collector de journald dos agentes;
5. normalizador NIP-AM por formato ACP, incluindo OpenCode;
6. correlação:

```text
event_id → channel → session_id → turn_id → harness thread
         → tool calls → response event → tokens/cost → outcome
```

7. evals Buzz × Deep durante a migração;
8. rate/budget por agente, canal e principal via broker.

Retenção inicial:

| Classe | Política inicial |
|---|---|
| journald | manter 256 MB / 7 dias |
| Docker JSON | 50 MB × 5 por container |
| NIP-AO local | 30 dias após implementar prune efetivo |
| NIP-AM | política por cliente antes do expurgo |
| harness local | idade + tamanho, separando cache de histórico |
| evals | campos mínimos, redação e retenção por cliente |

## P6 — wrapper e produto

O wrapper reutiliza contratos do Buzz:

- relay/comunidade/canais;
- identidade e NIP-OA;
- Activity;
- model/effort controls;
- personas/teams;
- broker/capabilities;
- cadastro de cases e preflight da aplicação jurídica.

Branding, composição e módulos específicos ficam em uma camada de distribuição,
sem alterar protocolos nem duplicar o core. Upgrades do Buzz entram por merge
controlado com testes de contrato.

## Gates de liberação

### Validação interna

- Activity e NIP-AM E2E;
- AGENTS/venv/corpus preflight;
- sessão persistente;
- matriz dos três harnesses.

### Escritório piloto

- allowlist de advogados;
- broker e grants por case;
- logs/evals/FinOps;
- backup/restore testado.

### Usuário público

- processo sem autoridade privada compartilhada;
- policy DM ou canal público isolado;
- capabilities self-only;
- budget/rate limit/anti-spam;
- segregação e tentativa de exfiltração testadas;
- requester activity definida ou explicitamente não oferecida.

Nenhum gate pode ser satisfeito apenas por system prompt.

# Trace comparativo — pergunta de data/hora

Data: 19 de setembro de 2026
Pergunta: `Q HORAS E DIA SAO`

## Buzz / Juri Root

Superfície: DM Buzz
Canal interno: `ac25c73b-7925-4dc5-a2ff-0cef806e5a43`
Evento humano: `455b396625e068376bb2ebc45a383459bb3e1a25e41abbcdc04ca12793fa1887`

```text
11:20:21  mensagem humana criada no relay
11:20:24  contexto resolvido / turno Codex iniciado
11:20:25  userMessage entrou na thread Codex
11:20:28  reasoning: preparar resposta com hora exata
11:20:39  `date` concluído
11:20:40  reasoning: preparar publicação Buzz
11:20:49  `buzz messages send` concluído; resposta visível no Buzz
11:20:57  turno ACP/Codex concluído
```

Resultado:

```text
Hoje é sábado, 19/09/2026, às 11h20 (horário de Brasília).
```

Configuração:

```text
thread_id: 01a0ba0a-2d46-7912-ace8-dbd14b610be6
turn_id:   01a0ba0a-2e5e-7973-a05e-a71304e99849
model:     gpt-5.6-luna
effort:    max
modo:      agent-full-access
sessão:    nova sessão da DM
```

Métricas:

```text
ingress até início Codex: ~3 s
início Codex até resposta visível: ~25 s
evento humano até resposta visível: 28 s
turno Codex completo: 32,913 s
evento humano até fim: 36 s
tools: 2 (`date` + publicação Buzz)
```

## Codex Desktop direto no repositório

Superfície: Codex Desktop, thread já existente no repositório jurídico.

```text
11:21:34  turno iniciado
11:21:39  userMessage persistida
11:21:44  reasoning: obter hora local exata
11:21:45  comentário de progresso
11:21:46  tool de hora/web iniciada
11:21:48  tool concluída (~1,5 s)
11:21:50  resposta final visível
11:21:51  turno concluído
```

Resultado:

```text
Hoje é sábado, 19 de setembro de 2026. Agora são aproximadamente 11h21,
no horário de Brasília/São Paulo (UTC−3).
```

Configuração:

```text
thread_id: 01a0a0fe-7935-7c02-8cfe-731708f9a686
turn_id:   01a0ba0b-3fb5-7ea1-a125-046e647931de
model:     gpt-5.6-sol
effort:    high
modo:      full access
sessão:    thread existente
```

Métricas:

```text
turno completo: 17,017 s
userMessage até resposta visível: ~11 s
tools: 1 (hora via tool/web)
contexto acumulado reportado: ~151,9 mil tokens
houve uma reconexão rápida do Responses WebSocket
```

## Explicação da diferença

O relay não explica a diferença. O Juri pagou:

1. criação da primeira sessão da DM;
2. Luna com effort `max`;
3. um ciclo para decidir consultar a hora;
4. um ciclo após `date` para construir a publicação;
5. a publicação explícita via `buzz messages send`;
6. um ciclo final interno do ACP depois da publicação.

O Codex Desktop usou uma thread quente, Sol High, uma única tool integrada e
entregou a resposta final diretamente à própria UI. No Buzz, a resposta só
existe no canal quando o agente executa a tool de publicação.

Otimizações sem alterar a arquitetura:

- manter o binding da sessão após restart;
- usar effort menor quando escolhido pelo owner na UI;
- evitar tool externa para data/hora quando o contexto já trouxer timestamp
  confiável suficiente;
- manter o base prompt compacto;
- medir `publish-visible` separadamente de `turn-completed`.

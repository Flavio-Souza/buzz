# Observabilidade do Buzz no VPS

Este diretório é um projeto Compose opcional e separado. Ele não altera o
protocolo, o relay ou os clientes Buzz. A única ligação com o projeto principal
é a rede externa `buzz-vps_buzz-net`.

## Fluxo

```text
Buzz relay /metrics (:9102 interno)
  └── Prometheus ── Grafana

Buzz relay OTLP/gRPC
  └── Tempo ─────── Grafana

systemd agents ───────────────┐
Docker logs ── bridge host ───┴─ journald ── Alloy ── Loki ── Grafana
```

Alloy seleciona apenas `buzz-vps.service`, `buzz-agent-*.service`, instâncias
`buzz-agent@*.service` e o bridge opcional `buzz-container-logs.service`. Ele
não lê journals de serviços da Pleroma.

Os logs Docker do relay, MinIO e da própria stack continuam no driver
`json-file`, com rotação. Não montamos o socket Docker no Alloy: a flag
`read_only` de um bind mount não transforma a API Docker em read-only. Para
centralizar relay e MinIO no Loki, um unit host-side restrito executa somente
`docker compose logs` e encaminha seu stdout ao journald.

## Portas

| Serviço | Porta do host | Exposição |
|---|---:|---|
| relay/web | 3300 | `BUZZ_BIND_HOST`, definido no Compose principal |
| Prometheus | 3310 | `BUZZ_BIND_HOST` / WireGuard |
| Grafana | 3311 | `BUZZ_BIND_HOST` / WireGuard, login obrigatório |
| Loki | nenhuma | somente `buzz-net` |
| Tempo HTTP/OTLP | nenhuma | somente `buzz-net` |
| Alloy | nenhuma | somente loopback do container |

Não publique Loki, Tempo ou Prometheus diretamente na internet. Prometheus não
possui autenticação nesta configuração e depende da fronteira WireGuard.

## Preparação

Copie as variáveis da seção de observabilidade de `../.env.example` para o
`../.env` existente. Gere uma senha exclusiva para o Grafana:

```bash
cd /home/ubuntu/iaquant/LAB/buzz/deploy/vps
openssl rand -base64 36
chmod 600 .env
```

Grave o valor apenas em:

```env
BUZZ_GRAFANA_ADMIN_PASSWORD=<valor-gerado>
```

O Compose recusa iniciar o Grafana quando essa variável não existe. Não use a
senha de banco, MinIO, relay ou identidade Nostr.

Confirme também os grupos `systemd-journal` e `adm` no host. Os defaults desta
máquina são `999` e `4`:

```bash
getent group systemd-journal
getent group adm
```

Se o número for diferente, configure `BUZZ_SYSTEMD_JOURNAL_GID` no `.env`. O
Alloy roda como usuário não-root e recebe somente esses grupos suplementares
para ler o journal.

Instale o bridge de logs Docker no host. Ele não contém segredo e não altera os
containers:

```bash
sudo install -m 0644 observability/buzz-container-logs.service \
  /etc/systemd/system/buzz-container-logs.service
sudo systemctl daemon-reload
sudo systemctl enable --now buzz-container-logs.service
```

O processo roda como `ubuntu`, que já opera o Compose, e só solicita os logs de
`relay`, `minio` e `minio-init`. O socket Docker permanece fora do container do
Alloy.

## Validar e iniciar

```bash
cd /home/ubuntu/iaquant/LAB/buzz/deploy/vps

docker compose --env-file .env \
  -f observability/compose.yaml \
  config --quiet

docker compose --env-file .env \
  -f observability/compose.yaml \
  up -d --wait

docker compose --env-file .env \
  -f observability/compose.yaml \
  ps
```

O projeto principal deve estar ativo primeiro, pois é ele que cria a rede. Para
habilitar traces, descomente os valores abaixo no `.env` e faça uma recriação
controlada somente do relay:

```env
OTEL_EXPORTER_OTLP_ENDPOINT=http://tempo:4317
OTEL_SERVICE_NAME=buzz-relay
BUZZ_OTEL_FILTER=buzz_relay=info,buzz_datastore=info
```

```bash
docker compose --env-file .env -f compose.yaml up -d --force-recreate relay
```

`BUZZ_OTEL_FILTER` é independente de `RUST_LOG`: reduzir stdout não remove os
spans exportados e aumentar stdout não amplia automaticamente o tracing OTLP.

## Verificação

```bash
# Prometheus deve mostrar buzz-relay como UP.
curl --fail http://10.20.0.1:3310/-/ready

# Grafana deve responder e já possuir os três datasources provisionados.
curl --fail http://10.20.0.1:3311/api/health

# Logs do coletor e dos backends.
docker compose --env-file .env \
  -f observability/compose.yaml \
  logs --tail 100 alloy loki tempo prometheus grafana

# Bridge Docker → journald → Alloy → Loki.
systemctl status buzz-container-logs.service --no-pager
```

No Grafana:

- Explore → Prometheus: consulte `up{job="buzz-relay"}`;
- Explore → Loki: consulte `{job="systemd-journal"}`;
- Explore → Tempo: procure traces recentes do serviço `buzz-relay`.

## Retenção e recursos

| Componente | Política inicial |
|---|---|
| Prometheus | `15d`, limitado também a `5GB` |
| Loki | `336h` (14 dias) |
| Tempo | `72h` |
| Alloy | posições de leitura apenas; logs ficam no Loki |
| Grafana | configuração e preferências persistentes |

Os containers possuem rootfs read-only, `no-new-privileges`, capabilities
removidas, limites de PIDs e limites iniciais de memória. Ajustes de memória
podem ser feitos no `.env` pelas variáveis `BUZZ_*_MEMORY_LIMIT` sem editar o
Compose.

As retenções reduzem risco de disco e exposição de dados operacionais. Elas não
constituem backup nem arquivo jurídico. Eventos Buzz continuam no Postgres,
mídia no MinIO e auditoria de domínio no `buzz-audit`.

## Parar somente a observabilidade

Como a stack possui seu próprio projeto Compose, `down` remove somente seus
containers e sua rede lógica. A rede compartilhada é externa e permanece sob o
projeto principal:

```bash
docker compose --env-file .env \
  -f observability/compose.yaml down
```

Enquanto o relay mantiver `OTEL_EXPORTER_OTLP_ENDPOINT=http://tempo:4317`, ele
tentará exportar para o Tempo parado. Para remover o overlay completamente,
reconverja o relay apenas com o Compose base:

```bash
docker compose --env-file .env -f compose.yaml up -d --force-recreate relay
```

Os volumes permanecem. Remova-os somente após backup e decisão explícita.

## Exporter OTLP externo

É possível usar um collector externo sem iniciar este overlay. Defina no `.env`
um endpoint alcançável de dentro do container do relay:

```env
OTEL_EXPORTER_OTLP_ENDPOINT=https://collector.exemplo:4317
OTEL_SERVICE_NAME=buzz-relay
BUZZ_OTEL_FILTER=buzz_relay=info,buzz_datastore=info
```

O Compose base usa `env_file: .env`, portanto essas variáveis são encaminhadas
ao relay. Tokens ou headers do exporter devem permanecer no `.env` ou no secret
manager do deployment; nunca nos YAMLs versionados.

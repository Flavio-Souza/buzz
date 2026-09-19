# Buzz no VPS via WireGuard

O relay oficial do Buzz roda em container Docker, gerenciado pelo Compose em
`compose.yaml`. Postgres e Redis são serviços existentes no host `db`; o
armazenamento S3 é um MinIO dedicado do Buzz, com dados em
`deploy/vps/data/minio`. Este deployment não cria banco ou Redis local. O Garage
existente permanece separado para os serviços que já o utilizam.

A família de portas do Buzz é `33xx`, fora das portas presentes no registry da
Pleroma. O relay e o frontend web escutam somente em `10.20.0.1:3300`, na
interface WireGuard. No Desktop,
adicione o relay `ws://10.20.0.1:3300` enquanto seu computador estiver conectado
à VPN. O canal inicial é `geral` e sua identidade existente é a dona do relay.

Arquitetura detalhada, fluxo ACP, isolamento de `HOME` e operação de agentes:
[ARCHITECTURE.md](./ARCHITECTURE.md).

## Interface web

O mesmo bundle oficial inclui um frontend web para convites e navegação de
repositórios. Com `BUZZ_SERVE_GIT_WEB_GUI=true`, ele fica disponível em
`http://10.20.0.1:3300` e em `/repos`. Essa interface web não substitui ainda a
experiência completa de canais e agentes do Desktop; ambas usam o mesmo relay e
a mesma identidade.

## Operação

```bash
cd /home/ubuntu/iaquant/LAB/buzz/deploy/vps
docker compose --env-file .env -f compose.yaml ps
docker compose --env-file .env -f compose.yaml logs -f relay
sudo systemctl status buzz-vps
sudo systemctl restart buzz-vps
```

`buzz-vps.service` inicia após Docker e WireGuard. O container tem
`restart: unless-stopped`; os serviços externos ficam fora deste projeto. Não
use `docker compose down -v` nesta instalação.

Os logs locais do relay e do MinIO usam `json-file` com rotação. Os defaults
são cinco arquivos de 50 MB por container e podem ser ajustados em `.env` com
`BUZZ_DOCKER_LOG_MAX_SIZE` e `BUZZ_DOCKER_LOG_MAX_FILES`. A alteração só passa a
valer depois que o Compose recria o container.

Segredos ficam em `.env` com permissão `0600`, fora do Git. As imagens do relay e
MinIO são fixadas por digest no Compose/.env para upgrades explícitos. Faça
backup do `.env`, do Postgres de `db`, de `deploy/vps/data/minio` e de
`deploy/vps/data/git` antes de trocar a versão.

Mensagens e eventos assinados ficam no Postgres. MinIO guarda mídia e objetos
Git. O probe CAS do Buzz fica habilitado contra MinIO; isso valida a escrita
condicional exigida pelo suporte Git antes de o relay aceitar tráfego.

## Observabilidade opcional

O diretório [`observability`](./observability) contém um projeto Compose separado com
Prometheus, Grafana, Loki, Tempo e Alloy. Ele usa as superfícies nativas do
Buzz: Prometheus coleta `relay:9102`, o relay exporta spans por OTLP para Tempo,
e Alloy envia somente os journals dos units `buzz-agent*` e `buzz-vps` ao Loki.

Prometheus e Grafana são publicados apenas no endereço WireGuard, em `3310` e
`3311`. Loki, Tempo e a UI interna do Alloy não publicam portas no host. Antes
de iniciar, gere a senha do Grafana no `.env` privado:

```bash
printf 'BUZZ_GRAFANA_ADMIN_PASSWORD=%s\n' "$(openssl rand -base64 36)"
# copie a linha para deploy/vps/.env, que permanece modo 0600 e fora do Git

docker compose --env-file .env -f observability/compose.yaml up -d --wait

# Depois de habilitar OTEL_EXPORTER_OTLP_ENDPOINT=http://tempo:4317 no .env:
docker compose --env-file .env -f compose.yaml up -d --force-recreate relay
```

A stack conecta na rede externa criada pelo projeto `buzz-vps`, mas possui ciclo
de vida e volumes próprios. O segundo comando recria somente o relay para
configurar seu exporter OTLP nativo. Os endereços na VPN ficam:

```text
Prometheus  http://10.20.0.1:3310
Grafana     http://10.20.0.1:3311
```

Configuração, retenção, operação e limites de segurança estão documentados em
[`observability/README.md`](./observability/README.md).

## Agentes no host

O relay/MinIO continuam no Compose; Codex e Claude Code rodam no host sob
systemd, usando os adapters ACP instalados em `/home/ubuntu/.local/bin`. O
serviço `buzz-agent@.service` é um template: cada instância recebe seu próprio
processo, `HOME`, identidade, prompt e workspace.

Cada agente tem arquivos e diretórios no host:

```text
agents/<nome>/agent.env       # segredos e configuração, modo 0600
agents/<nome>/instructions.md # prompt editável, montado somente leitura
agents/<nome>/home/           # estado do runtime
agents/<nome>/workspace/      # arquivos de trabalho
```

O script `agent.sh` prepara e opera instâncias:

```bash
cd /home/ubuntu/iaquant/LAB/buzz/deploy/vps
./agent.sh create assistente
# Gere uma identidade própria para o agente.
./agent.sh keygen assistente
# Copie a chave secreta de identity.txt para BUZZ_PRIVATE_KEY.
# Opcional: copie a sessão já autenticada do host para o HOME isolado:
./agent.sh auth assistente codex
# Edite agent.env e instructions.md; escolha codex-acp ou claude-agent-acp.
# O owner humano deve ser membro direto do relay. Para o agente, gere um
# BUZZ_AUTH_TAG NIP-OA assinado pelo owner e configure-o em agent.env.
# NÃO adicione a pubkey do agente como membro direto; isso impede o relay de
# materializar agent_owner_pubkey e quebra NIP-AO/NIP-AM.
./agent.sh start assistente
./agent.sh logs assistente
```

Para criar outro agente, repita `create` com outro nome. Cada agente precisa de
uma chave Nostr própria e de um `BUZZ_AUTH_TAG` NIP-OA emitido por um owner que
seja membro direto do relay. A pubkey do agente não entra no roster direto. O
provedor/modelo fica configurado em `agent.env`. Os adapters ACP são `codex-acp` e
`claude-agent-acp`; o README de `crates/buzz-acp` documenta o contrato e o
README de cada adapter documenta sua autenticação.

O relay usa uma bridge Docker: publica somente `10.20.0.1:3300` e alcança `db`
pela WireGuard; MinIO fica apenas na rede interna do Compose. Agentes host-side
usam o relay publicado em `10.20.0.1:3300`. As diretivas systemd restringem
escrita ao `home` e `workspace` do agente; como o processo roda como `ubuntu`,
ele ainda pode ler outros arquivos aos quais esse usuário tenha acesso.

Para agentes que tratam dados sensíveis ou conversam com terceiros, use o
modelo do **Juri Root**: usuário Linux dedicado, diretório em
`/srv/buzz-agents/<nome>`, runtime ACP root-owned em `/opt/buzz-agent-runtime`
e `ProtectHome=yes`. O unit instalado é `buzz-agent-juri-root.service`; a
arquitetura e os comandos operacionais estão em
[ARCHITECTURE.md](./ARCHITECTURE.md#home-isolado).

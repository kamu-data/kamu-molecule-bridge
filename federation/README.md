# Federation

## All-in-one startup

⚠️ Under construction: use the next section steps instead. For details see [docker-compose.yml](./docker-compose.yml).

```shell
docker compose --profile all up
```

### Simulate supergraph hot-reload update

With pre-launched services, execute:

```shell
docker compose --profile supergraph-compose up
```

## Running services for debugging 

1. (1-st terminal) Run Kamu Core API in the workspace you want to work with:

Example:
```shell
# Security: localhost GitHub OAuth application credentials 
docker run -t --rm \
  -p 8080:8080 \
  -e KAMU_AUTH_GITHUB_CLIENT_ID=361a3b4fda86d0234d2f \
  -e KAMU_AUTH_GITHUB_CLIENT_SECRET=465849325236ed49253993744069e1bec6808554 \
  ghcr.io/kamu-data/kamu-base:latest-with-data-mt \
  kamu system api-server --address 0.0.0.0 --http-port 8080
```

2. (2-nd terminal) Run Kamu Molecule Bridge:

🗒️ If you haven't filled `.env` earlier (in the repo root), now's the time.

```shell
KAMU_MOLECULE_BRIDGE_HTTP_ADDRESS=0.0.0.0 \
KAMU_MOLECULE_BRIDGE_HTTP_PORT=8081 \
  cargo run -- run --dry-run
```

3. Router launch.

3.1. (3-rd terminal) Before running Router, we need to prepare a supergraph schema:

```shell
docker run -t --rm \
  --network host \
  -v ./supergraph:/supergraph \
  -e APOLLO_ELV2_LICENSE=accept \
  worksome/rover:0.35.0 \
  supergraph compose --config /supergraph/config.yaml -o /supergraph/schema.graphql
```

3.2. (3-rd terminal) Run Router:

```shell
docker run -t --rm \
  --network host \
  -v ./supergraph:/supergraph \
  -e APOLLO_TELEMETRY_DISABLED=true \
  ghcr.io/apollographql/router:v2.8.0 \
  --supergraph /supergraph/schema.graphql --dev
```

3.3. Open http://localhost:4000/graphql to access GraphQL playground.

### Get subgraph schema with federation directives

The schema for GQL API users doesn't contain federation directives for simplicity. However, for debugging superschema
composing errors, we need to see the exact schema that the Router receives.

```shell
docker run -t --rm \
  --network host \
  -v ./supergraph:/supergraph \
  -e APOLLO_ELV2_LICENSE=accept \
  worksome/rover:0.35.0 \
  subgraph introspect http://localhost:8080/graphql > subgraph_8080.graphql
```

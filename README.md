# Kamu-Molecule Bridge Component
Assigns users correct access permissions based on IPT/IPNFT information indexed from blockchain.


## High-level Overview

**Inputs**:
- IPNFT contract events ([ABI](./src/infra/molecule_contracts/abis/IPNFT.json)):
  - `IPNFTMinted`
  - `Transfer`
- Tokenizer contract events ([ABI](./src/infra/molecule_contracts/abis/Tokenizer.json)):
  - `TokensCreated`
- IPToken contract events ([ABI](./src/infra/molecule_contracts/abis/IPToken.json)):
  - `Transfer` (ERC20)
- Safe multisig wallet contract events ([ABI](./src/infra/molecule_contracts/abis/Safe.json)):
  - `AddedOwner`
  - `RemovedOwner`
- Safe multisig wallet API:
  - Used to fetch the list of wallet owners
- Kamu Node:
  - `molecule/projects` (or as configured) dataset is used to discover project accounts that were created in Kamu
  - `<project-account>/data-room` datasets are scanned to discover the set of datasets and files that projects which to expose to the investors and community
  - `<project-account>/<dataset-id>` datasets referenced in `data-room` are scanned for `molecule_access_permissions` to understand to which category of users the access should be provided

**Outputs**:

Service uses Kamu Node API to set the access permissions for relevant accounts and token holders.

**Catch-up phase**:

- Blockchain: Indexing to have complete information about all IPNFTs, IPTokens and their owners/holders
- API: Loading allowlisted projects from the `molecule/projects` dataset
- API: Loading versioned files associated with projects (via data-rooms)
- API: Loading and tracking `molecule_access_level` for later access permissions assignment
- Bridge: Complete granting of access permissions for all owners and holders.

**Update loop**:

- Blockchain: Periodic (configurable) indexing of new blocks.
- API: Periodic (configurable) querying of dataset changes associated with projects.
- Bridge: Granting/revoking access according to blockchain and dataset changes:
  - Changed IPNFT owners / or changing multisig participants
  - New holders / Holders without the required balance
  - Added / removed files

## Developing
See [`DEVELOPER.md`](./DEVELOPER.md) for developer instructions.


## Deploying
Bridge is a **stateful long-running service** built to be deployed into Kubernetes.

See [Helm chart repo](https://github.com/kamu-data/kamu-molecule-bridge-helm-charts) for deployment artifacts.

**Dependencies**:
* EVM JSONRPC Node URL - used to index the state of the contracts from blockchain
* Kamu Node URL - used to discover the project data rooms structure and assign necessary access permissions to token holders.

## Configuring
The service accepts both environment variables (via `.env`) and a `config.yaml` file (location can be specified via CLI arguments). 

> [!NOTE]
> Environment variables take precedence over the config.

See [`.env.example`](./.env.example) for sample configuration.


## Monitoring
The service provides the following monitoring features:

**Structured logging** via `tracing` crate:
- In development mode the logs are human-readable, but in production deployment the logs are emitted in ND-JSON format
- The logs are always directed to `stderr`
- It is advised that in production deployment the pod output is directed into a log collector like Loki or Elasticsearch

**Health checks**:
- Application supports a full set of checks (*startup, readiness, liveness*) used by Kubernetes
- The supplied Helm chart exposes them via `/system/health` HTTP endpoint

**Prometheus metrics**:
- Application reports metrics on the number of RPC requests executed, error encountered etc.
- Metrics are exposed via `/system/metrics` HTTP endpoint
- The supplied Helm chart configures supports enabling `ServiceMonitor` CRD to allow Prometheus Operator in the cluster to automatically start scraping the metrics

**Alerting**:
- Alerts can be easily set up to react to abnormal values in Prometheus metrics
- The supplied Helm chart supports `PrometheusRule` CRD to define and customize alerting conditions

**OpenTelemetry tracing** via `opentelemetry` crate:
- Information from `tracing` crate is also directed to OTEL to capture execution flow of operations that can be visualized by tracing front-ends like Tempo and Jagger
- OTEL collector needs to be specified via `KAMU_OTEL_OTLP_ENDPOINT` env var
- OTEL integration is supported by the supplied Helm chart but requires collectors to be set up by the Kubernetes cluster maintainer


## Troubleshooting

**Indexer state**:

Service provides `/system/state` endpoint that returns the projected state of what permissions should be given to which accounts as indexed from the blockchain.

**Re-Synchronization**:

In the event of a bug or manual changes in access permissions in Kamu Node it may sometimes be necessary to re-synchronize the blockchain state with permissions in Kamu from scratch. To achieve this, just restart the service.

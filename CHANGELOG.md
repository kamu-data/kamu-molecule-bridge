# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!--
Recommendation: for ease of reading, use the following order:
- Added
- Changed
- Fixed
-->

## [0.5.3] - 2026-01-28
### Fixed
- `MoleculeAccessLevel::Holder`: extended to support plural values encountered on testnet.

## [0.5.2] - 2026-01-28
### Changed
- HTTP: Added exponential backoff on errors (#44).
- GQL: Update kamu-api-server schema (#44).
- Updated some dependencies (#44):
  - `alloy` to `1.5.2`
  - `aws-lc-rs` to `1.15.4`
### Fixed
- Access revocation for deleted/disabled projects (`-R`, retraction) (#44).
- Config (env vars): `KAMU_MOLECULE_BRIDGE_IGNORE_PROJECTS_IPNFT_UIDS` correct parsing of list values (#44).
- Increased the number of rows requested in SQL query results (#44).

## [0.5.1] - 2026-01-26
### Fixed
- Updating the SQL query for the list of versioned files after updating Kamu Node version to 0.75.1 (#30):
  - Added a workaround so that the query works correctly with Datafusion 50, 
    which began to be used with the new release.

## [0.5.0] - 2025-08-28
### Added
- `kamu-molecule-bridge run --dry-run`: Mode in which Bridge does not make any changes to Kamu Node (#29).
### Changed
-  Multisig indexing improvements (#29).

## [0.4.0] - 2025-08-01
### Changed
- Stabilizing block interval indexing when they contain many events (#25).

## [0.3.0] - 2025-07-30
### Added
- New `ignore_projects_ipnft_uids` config option that allows us to exclude certain projects from synchronization.

## [0.2.1] - 2025-07-23
### Fixed
- Config: `molecule_projects_loading_interval_in_secs`: removed extra prefix.

## [0.2.0] - 2025-07-23
### Added
- Added a configurable interval for querying project changes during cyclic indexing.
- API: `GET /system/state`: Added logging of applied operations.
- `SafeWalletApiService`: cheap blockchain calls before expensive Safe Transaction API (HTTP) calls.
### Changed
- Improved iterative indexing.
- `SafeWalletApiService`: result caching for regular addresses as well.
- `MoleculeAccessLevel`: can be parsed from both upper/lower cases.

## [0.1.9] - 2025-07-21
### Fixed
- Errors during RPC initialization will be properly directed to tracing.
- Enabled debug info in release builds for ease of troubleshooting.

## [0.1.8]
### Changed
- Improved Prometheus metrics.
### Fixed
- Optimized SQL query that fetches file access levels.

## [0.1.7]
### Changed
- Unlocked mutation GQL calls.

## [0.1.6]
### Changed
- Filter empty versioned files.

## [0.1.5]
### Changed
- No changes, new version to remove collision with helm-chart repository.

## [0.1.4]
### Added
- RPC: Initial retry calls logic.
### Changed
- Do not request `molecule_access_level` for removed versioned files.
### Fixed
- Infura: handle RPC gateway timeouts.

## [0.1.0]
### Added
- Initial bridge version that indexes the state from blockchain but does not apply it to the Kamu Node yet.

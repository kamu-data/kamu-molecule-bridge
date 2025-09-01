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

## [0.5.0] - 2025-08-28
### Added
- (#29) `kamu-molecule-bridge run --dry-run`: Mode in which Bridge does not make any changes to Kamu Node.
### Changed
- (#29) Multisig indexing improvements.

## [0.4.0] - 2025-08-01
### Changed
- (#25) Stabilizing block interval indexing when they contain many events.

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

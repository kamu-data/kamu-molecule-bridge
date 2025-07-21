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

## [0.1.8]
### Changed
- Improved Prometheus metrics
### Fixed
- Optimized SQL query that fetches file access levels

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
- Initial bridge version that indexes the state from blockchain but does not apply it to the kamu node yet.

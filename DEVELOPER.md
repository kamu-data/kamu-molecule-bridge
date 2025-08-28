# Developer Guide <!-- omit in toc -->


## Making Changes
This repo is a fairly standard rust project with multiple crates combined into a single workspace.

Rustc version is pinned in the `rust-toolchain.toml` file for build reproducibility.

The `build` CI action in GitHub will trigger on every commit to run linting and tests.

### Updating ABI

Going into the [IPNFT repository](https://github.com/moleculeprotocol/IPNFT), you need to generate ABI using actual code of smart contracts. 
To achieve this, you will need the following commands:

```shell
# cd IPNFT repo
forge inspect src/IPNFT.sol abi --json > IPNFT.json
forge inspect src/IPToken.sol abi --json > IPToken.json
forge inspect src/Tokenizer.sol abi --json > Tokenizer.json

# cd safe-contracts repo (https://github.com/safe-global/safe-smart-account)
git checkout release/v1.3.0
forge inspect ./contracts/GnosisSafe.sol abi --json > Safe_1.3.0.json # not indexed events

git checkout release/v1.5.0
forge inspect ./contracts/Safe.sol abi --json > Safe_1.5.0.json # indexed events
```

After a successful generation, update the changed contracts' ABI in this repository. 
All other Rust codegen will happen automatically on project rebuild.

⚠️ NB. Always save ABI in the repository. If event signatures change, a new service version is required.

## Release Procedure
Prerequisites:
```sh
cargo install cargo-edit
```

To create a new release, follow these steps:
1. Merge desired branches into `master`
2. Increment the version of the crates:
   ```sh
   cargo set-version --workspace MAJOR.MINOR.PATCH
   ```
3. Update [CHANGELOG.md](./CHANGELOG.md)
4. Commit the release prep changes:
   ```sh
   git add .
   git commit -m vMAJOR.MINOR.PATCH
   ```
5. Tag the commit:
   ```sh
   git tag vMAJOR.MINOR.PATCH
   ```
6. Push release commit and tag.
7. This tag will kick off the `release` CI action that will build the binaries, create a release in GitHub, and attach binaries to the release
8. Upon its completion the `release-images` CI action will build and publish the OCI image as a package on https://ghcr.io
9. The image is now ready to be used in the [Helm chart](https://github.com/kamu-data/kamu-molecule-bridge-helm-charts)

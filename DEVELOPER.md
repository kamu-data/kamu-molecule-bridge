# Developer Guide <!-- omit in toc -->


## Release Procedure
Prerequisites:
```sh
cargo install cargo-edit
```

To create a new release follow these steps:
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
7. This tag will kick-off the `release` CI action that will build the binaries, create a release in GitHub, and attach binaries to the release
8. Upon its completion the `release-images` CI action will build and publish the OCI image as a package on https://ghcr.io
9. The image is now ready to be used in the [Helm chart](https://github.com/kamu-data/kamu-molecule-bridge-helm-charts)

IMAGE_REPO = ghcr.io/kamu-data
APP_VERSION = $(shell cargo metadata --format-version 1 | jq -r '.packages[] | select( .name == "kamu-molecule-bridge") | .version')


.PHONY: download
download:
	mkdir -p artifacts
	gh release download v$(APP_VERSION) \
		--repo kamu-data/kamu-molecule-bridge \
		--pattern "kamu-molecule-bridge-x86_64-unknown-linux-gnu.tar.gz" \
		--dir artifacts


.PHONY: build
build:
	docker build \
		--build-arg APP_VERSION=$(APP_VERSION) \
		-t $(IMAGE_REPO)/kamu-molecule-bridge:$(APP_VERSION) \
		.


.PHONY: push
push:
	docker push $(IMAGE_REPO)/kamu-molecule-bridge:$(APP_VERSION)


.PHONY: clean
clean:
	rm -rf artifacts

IMAGE_REPO = ghcr.io/kamu-data
APP_VERSION = $(shell cargo metadata --format-version 1 | jq -r '.packages[] | select( .name == "kamu-molecule-bridge") | .version')


.PHONY: build
build:
	docker build \
		--build-arg APP_VERSION=$(APP_VERSION) \
		-t $(IMAGE_REPO)/kamu-molecule-bridge:$(APP_VERSION) \
		.


.PHONY: push
push:
	docker push $(IMAGE_REPO)/kamu-molecule-bridge:$(APP_VERSION)

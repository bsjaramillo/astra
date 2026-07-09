# Makefile de Astra
# Ejecutá `make` o `make help` para ver las tareas disponibles.

CARGO   := cargo
BIN     := astra
IMAGE   := ghcr.io/bsjaramillo/astra
TAG     ?= local
PREFIX  ?= $(HOME)/.local
CONFIG  ?= astra.toml

.DEFAULT_GOAL := help

## help: muestra esta ayuda
.PHONY: help
help:
	@echo "Astra — tareas disponibles:"
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/## /  /'

## build: compila el workspace en modo debug
.PHONY: build
build:
	$(CARGO) build --workspace

## release: compila el binario optimizado (target/release/$(BIN))
.PHONY: release
release:
	$(CARGO) build --release --bin $(BIN)
	@echo "Binario: target/release/$(BIN) ($$(du -h target/release/$(BIN) | cut -f1))"

## run: corre el server (usa CONFIG=archivo.toml para config custom)
.PHONY: run
run:
	$(CARGO) run --bin $(BIN) -- --config $(CONFIG)

## test: corre toda la suite de tests
.PHONY: test
test:
	$(CARGO) test --workspace

## bench: corre los benchmarks (criterion) de proto-ares
.PHONY: bench
bench:
	$(CARGO) bench -p proto-ares

## fmt: formatea el código
.PHONY: fmt
fmt:
	$(CARGO) fmt

## lint: corre clippy con warnings como errores
.PHONY: lint
lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

## check: fmt (verificación) + clippy + tests (como en CI)
.PHONY: check
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace

## install: instala el binario en $(PREFIX)/bin (PREFIX=/usr/local para global)
.PHONY: install
install: release
	install -Dm755 target/release/$(BIN) $(PREFIX)/bin/$(BIN)
	@echo "Instalado en $(PREFIX)/bin/$(BIN)"

## docker: construye la imagen local ($(IMAGE):$(TAG), default TAG=local)
.PHONY: docker
docker:
	docker build -t $(IMAGE):$(TAG) .
	@echo "Imagen: $(IMAGE):$(TAG)"

## docker-run: corre la imagen local en el puerto 5009
.PHONY: docker-run
docker-run:
	docker run --rm -p 5009:5009 -p 5009:5009/udp $(IMAGE):$(TAG)

## clean: limpia los artefactos de compilación
.PHONY: clean
clean:
	$(CARGO) clean

## tag: crea y pushea un tag de release (uso: make tag VERSION=v0.1.0)
##      dispara el workflow que publica binarios multi-arch + imágenes Docker.
.PHONY: tag
tag:
	@test -n "$(VERSION)" || { echo "Uso: make tag VERSION=v0.1.0"; exit 1; }
	@echo "$(VERSION)" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+' || { echo "VERSION debe tener forma vMAJOR.MINOR.PATCH"; exit 1; }
	@git diff --quiet || { echo "Hay cambios sin commitear; commiteá antes de taggear."; exit 1; }
	git tag -a "$(VERSION)" -m "Release $(VERSION)"
	git push origin "$(VERSION)"
	@echo "Tag $(VERSION) pusheado. El workflow publicará binarios e imágenes Docker."

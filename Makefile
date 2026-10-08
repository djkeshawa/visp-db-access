.PHONY: dev web build test lint docker demo
dev:
	cargo run -p vda-server -- serve
web:
	cd web && npm ci && npm run build
build: web
	cargo build --release -p vda-server
test:
	cargo test --workspace
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cd web && npm ci && npm run lint
docker:
	docker build -t visp-db-access:local .
demo:
	docker compose --profile demo up --build

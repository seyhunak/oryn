# ORYN developer commands. `make help` lists them.
SHELL := /bin/bash
.DEFAULT_GOAL := help

-include .env
export

PG_PORT     ?= 55433
REDIS_PORT  ?= 56380
DATABASE_URL ?= postgres://oryn:oryn@localhost:$(PG_PORT)/oryn
REDIS_URL   ?= redis://localhost:$(REDIS_PORT)
ORYN_SECRET_KEY ?= dev-secret-key-dev-secret-key-0123456789
ORYN_WORKER_BOOTSTRAP_TOKEN ?= dev-worker-bootstrap-token
ORYN_LOCAL_IMPORT_ROOT ?= $(CURDIR)/examples
ORYN_ALLOW_PRIVATE_NETWORK ?= true
WEB_PORT    ?= 3000

.PHONY: help install deps dev dev-api dev-worker dev-web dev-runner dev-demo build test test-rust test-web test-runner test-demo lint fmt migrate seed demo up down logs openapi desktop desktop-dev clean

help: ## Show this help
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[1m%-12s\033[0m %s\n", $$1, $$2}'

install: ## Install toolchains' dependencies (Rust crates, web, runner, browsers)
	cargo fetch
	cd apps/web && pnpm install --frozen-lockfile
	cd adapters/playwright-runner && npm ci && npx playwright install chromium

deps: ## Start Postgres (pgvector) and Redis
	docker compose up -d --wait postgres redis

dev: deps ## Run API, worker, web, runner and demo app locally (Ctrl-C stops all)
	@trap 'kill 0' INT TERM EXIT; \
	$(MAKE) --no-print-directory dev-api & \
	sleep 4; \
	$(MAKE) --no-print-directory dev-runner & \
	$(MAKE) --no-print-directory dev-demo & \
	$(MAKE) --no-print-directory dev-worker & \
	$(MAKE) --no-print-directory dev-web & \
	wait

dev-api: ## Run the API (port 8080)
	ORYN_BIND=127.0.0.1:8080 ORYN_ARTIFACT_DIR=./data/artifacts ORYN_WORKSPACE_DIR=./data/repos cargo run -p oryn-api

dev-worker: ## Run a worker against the local API and runner
	ORYN_API_URL=http://localhost:8080 ORYN_RUNNER_URL=http://localhost:7400 ORYN_WORKER_TOKEN_FILE=./data/worker-token ORYN_WORKER_NAME=local-dev cargo run -p oryn-worker

dev-web: ## Run the web UI (port $(WEB_PORT))
	cd apps/web && ORYN_API_INTERNAL_URL=http://localhost:8080 pnpm exec next dev --port $(WEB_PORT)

dev-runner: ## Run the Playwright runner (port 7400)
	cd adapters/playwright-runner && npm run build && PORT=7400 node dist/server.js

dev-demo: ## Run the Northwind demo application (port 4000)
	cd examples/demo-application && PORT=4000 DEMO_FLAKY=1 node src/server.js

build: ## Build release binaries, the web app and the runner
	cargo build --release -p oryn-api -p oryn-worker
	cd apps/web && pnpm build
	cd adapters/playwright-runner && npm run build

test: test-rust test-web test-runner test-demo ## Run every test suite (needs `make deps`)

test-rust: deps ## Rust unit + integration tests (uses the database; AI is mocked)
	cargo test --workspace --locked

test-web: ## Web type check, lint and unit tests
	cd apps/web && pnpm lint && pnpm test

test-runner: ## Playwright runner tests
	cd adapters/playwright-runner && npm test

test-demo: ## Demo application tests
	cd examples/demo-application && npm test

lint: ## Format check + clippy + web lint
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cd apps/web && pnpm lint

fmt: ## Format Rust code
	cargo fmt --all

migrate: deps ## Apply database migrations
	cargo run -q -p oryn-db --bin oryn-migrate

seed: ## Seed the running API with the Northwind demo project
	node scripts/seed.mjs

demo: ## Seed and start one run of every demo suite
	node scripts/seed.mjs --run

up: ## Build and start the full stack in Docker (web on :3000)
	docker compose --profile app up -d --build --wait
	ORYN_URL=http://localhost:$${ORYN_API_PORT:-8080} DEMO_BASE_URL=http://demo-app:4000 node scripts/seed.mjs

down: ## Stop the Docker stack (keeps volumes)
	docker compose --profile app down

logs: ## Follow Docker logs
	docker compose --profile app logs -f --tail=100

openapi: ## Regenerate docs/api/openapi.json and the web client types from the running API
	curl -fsS http://localhost:8080/api/openapi.json | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.stringify(JSON.parse(s),null,2)))" > docs/api/openapi.json
	cd apps/web && pnpm api:types

desktop: ## Build the macOS app (apps/desktop/src-tauri/target/release/bundle)
	cd apps/desktop && npm ci && npx tauri build

desktop-dev: ## Run the macOS app against ORYN_SERVER_URL (default http://localhost:3000)
	cd apps/desktop && npm ci && npx tauri dev

clean: ## Remove build outputs
	cargo clean
	rm -rf apps/web/.next adapters/playwright-runner/dist apps/desktop/src-tauri/target data

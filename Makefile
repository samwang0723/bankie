.PHONY: help test test-coverage lint ci changelog-gen changelog-commit docker-build \
       local-setup local-infra local-init-db local-migrate local-build local-start local-worker local-gateway local-portal local-stop local-jwt local-demo local-e2e local-interactive local-core-test \
       docker-up docker-down docker-logs docker-clean docker-jwt docker-e2e docker-interactive docker-core-test

help: ## show this help
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_-]+:.*?## / {sub("\\\\n",sprintf("\n%22c"," "), $$2);printf "\033[36m%-25s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

PROJECT_NAME?=bankie
APP_NAME?=bankie
VERSION?=v0.1.0

APP_NAME_UND=$(shell echo "$(PROJECT_NAME)" | tr '-' '_')

SHELL = /bin/bash

ifneq (,$(wildcard .env))
    include .env
    export $(shell sed 's/=.*//' .env)
endif

##################
# local dev env  #
##################

# Default local dev credentials (override via .env or env vars)
DB_HOST      ?= localhost
DB_PORT      ?= 5432
DB_USER      ?= bankie_app
DB_NAME      ?= bankie_main
DB_PASSWD    ?= localpass
JWT_SECRET   ?= LocalDevSecretKey1234567890abcdefghijklmnop
SERVICE      ?= demo-service

DATABASE_URL ?= postgres://$(DB_USER):$(DB_PASSWD)@$(DB_HOST):$(DB_PORT)/$(DB_NAME)

local-setup: local-infra local-init-db local-migrate local-build local-jwt-gen local-start local-worker ## one-shot: infra + db + build + jwt + server + worker
	@echo ""
	@echo "=========================================="
	@echo "  Bankie is running on http://localhost:3030"
	@echo "=========================================="
	@echo ""
	@echo "JWT token written to: .local-jwt-token"
	@echo ""
	@echo "Quick test:"
	@echo "  curl -s http://localhost:3030/health"
	@echo ""
	@echo "Full demo (via Gateway):"
	@echo "  make local-gateway && make local-demo"
	@echo ""
	@echo "Core API test (direct):"
	@echo "  make local-core-test"
	@echo ""
	@echo "Stop everything:"
	@echo "  make local-stop"

local-infra: ## start PostgreSQL + Redis via docker-compose
	@echo "[local] Starting PostgreSQL + Redis..."
	@docker compose -f docker-compose.local.yml up -d
	@echo "[local] Waiting for PostgreSQL to be ready..."
	@for i in $$(seq 1 30); do \
		docker exec bankie-postgres pg_isready -U postgres > /dev/null 2>&1 && break; \
		sleep 1; \
	done
	@echo "[local] Waiting for Redis to be ready..."
	@for i in $$(seq 1 15); do \
		docker exec bankie-redis redis-cli ping 2>/dev/null | grep -q PONG && break; \
		sleep 1; \
	done
	@echo "[local] Infrastructure ready."

local-init-db: ## create DB user + database (idempotent)
	@echo "[local] Initializing database user and schema..."
	@PGPASSWORD= psql -h $(DB_HOST) -p $(DB_PORT) -U postgres -tc \
		"SELECT 1 FROM pg_roles WHERE rolname='$(DB_USER)'" | grep -q 1 || \
		PGPASSWORD= psql -h $(DB_HOST) -p $(DB_PORT) -U postgres -c \
		"CREATE USER $(DB_USER) WITH ENCRYPTED PASSWORD '$(DB_PASSWD)'; ALTER ROLE $(DB_USER) WITH CREATEDB;"
	@PGPASSWORD= psql -h $(DB_HOST) -p $(DB_PORT) -U postgres -tc \
		"SELECT 1 FROM pg_database WHERE datname='$(DB_NAME)'" | grep -q 1 || \
		(PGPASSWORD= psql -h $(DB_HOST) -p $(DB_PORT) -U postgres -c \
		"CREATE DATABASE $(DB_NAME); GRANT ALL PRIVILEGES ON DATABASE $(DB_NAME) TO $(DB_USER); ALTER DATABASE $(DB_NAME) OWNER TO $(DB_USER);")
	@echo "[local] Database initialized."

local-migrate: ## run migrations using env-based connection
	@echo "[local] Running migrations..."
	@DATABASE_URL="$(DATABASE_URL)" cargo run --bin migrations
	@echo "[local] Migrations complete."

local-build: ## build the bankie server binary
	@echo "[local] Building bankie..."
	@SQLX_OFFLINE=true cargo build
	@echo "[local] Build complete."

local-jwt-gen: ## generate JWT and save to .local-jwt-token
	@echo "[local] Generating JWT for service '$(SERVICE)'..."
	@DB_PASSWD=$(DB_PASSWD) JWT_SECRET=$(JWT_SECRET) RUST_LOG=info \
		cargo run --bin bankie -- --mode jwt --service $(SERVICE) 2>&1 | \
		grep -oE 'eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+' | head -1 > .local-jwt-token
	@if [ -s .local-jwt-token ]; then \
		echo "[local] JWT saved to .local-jwt-token"; \
	else \
		echo "[local] WARNING: Could not extract JWT. Run manually:"; \
		echo "  DB_PASSWD=$(DB_PASSWD) JWT_SECRET=$(JWT_SECRET) cargo run --bin bankie -- --mode jwt --service $(SERVICE)"; \
	fi

local-jwt: local-jwt-gen ## alias: generate JWT token
	@cat .local-jwt-token 2>/dev/null || echo "(no token file found)"

local-start: ## start the bankie server (background)
	@echo "[local] Starting bankie server..."
	@DB_PASSWD=$(DB_PASSWD) JWT_SECRET=$(JWT_SECRET) ENV=local RUST_LOG=info \
		cargo run --bin bankie -- --mode server &
	@echo "[local] Waiting for server to be ready..."
	@for i in $$(seq 1 30); do \
		curl -sf http://localhost:3030/health > /dev/null 2>&1 && break; \
		sleep 1; \
	done
	@echo "[local] Server ready at http://localhost:3030"

local-worker: ## start the worker process (background, runs cron jobs)
	@echo "[local] Starting bankie-worker..."
	@DB_PASSWD=$(DB_PASSWD) JWT_SECRET=$(JWT_SECRET) ENV=local RUST_LOG=info \
		cargo run --bin bankie-worker &
	@echo "[local] Worker started (cron jobs running in background)"

local-gateway: ## start the gateway server (background, requires bankie running)
	@echo "[local] Starting gateway..."
	@DB_PASSWD=$(DB_PASSWD) JWT_SECRET=$(JWT_SECRET) ENV=local RUST_LOG=info \
		cargo run --bin bankie-gateway &
	@echo "[local] Waiting for gateway to be ready..."
	@for i in $$(seq 1 30); do \
		curl -sf http://localhost:4040/health > /dev/null 2>&1 && break; \
		sleep 1; \
	done
	@echo "[local] Gateway ready at http://localhost:4040"

local-portal: ## serve portal SPA locally (requires npm)
	@echo "[local] Starting portal SPA dev server..."
	@cd portal-spa && npm run dev &
	@echo "[local] Portal SPA at http://localhost:5173"

local-stop: ## stop server + worker + gateway + tear down infra
	@echo "[local] Stopping bankie server, worker, and gateway..."
	@-pkill -f "bankie.*--mode server" 2>/dev/null || true
	@-pkill -f "bankie-worker" 2>/dev/null || true
	@-pkill -f "bankie-gateway" 2>/dev/null || true
	@echo "[local] Stopping Docker containers..."
	@docker compose -f docker-compose.local.yml down
	@echo "[local] Stopped."

local-demo: ## run the full demo scenario via Gateway (auto-creates portal org + API key)
	@./scripts/demo.sh

local-core-test: ## run Core API tests directly (requires running server + JWT)
	@if [ ! -f .local-jwt-token ] || [ ! -s .local-jwt-token ]; then \
		echo "ERROR: No JWT token found. Run 'make local-setup' first."; \
		exit 1; \
	fi
	@./scripts/core-test.sh "$$(cat .local-jwt-token)"

local-e2e: ## run E2E test suite (requires running server + JWT)
	@if [ ! -f .local-jwt-token ] || [ ! -s .local-jwt-token ]; then \
		echo "ERROR: No JWT token found. Run 'make local-setup' first."; \
		exit 1; \
	fi
	@./scripts/e2e-test.sh "$$(cat .local-jwt-token)"

local-interactive: ## interactive console for manual API testing (via Gateway)
	@./scripts/interactive.sh

######################
# docker full stack  #
######################

docker-up: ## start full stack (postgres + redis + migrations + bankie + gateway + portal)
	@echo "[docker] Building and starting full stack..."
	@docker compose up -d --build
	@echo ""
	@echo "[docker] Waiting for services to be ready..."
	@for i in $$(seq 1 60); do \
		curl -sf http://localhost:$${APP_PORT:-3030}/health > /dev/null 2>&1 && break; \
		sleep 2; \
	done
	@for i in $$(seq 1 30); do \
		curl -sf http://localhost:$${GATEWAY_PORT:-4040}/health > /dev/null 2>&1 && break; \
		sleep 2; \
	done
	@echo ""
	@echo "=========================================="
	@echo "  Bankie Core:    http://localhost:$${APP_PORT:-3030}"
	@echo "  Gateway API:    http://localhost:$${GATEWAY_PORT:-4040}"
	@echo "  Portal SPA:     http://localhost:$${PORTAL_PORT:-8080}"
	@echo "=========================================="
	@echo ""
	@echo "View logs:  make docker-logs"
	@echo "Stop:       make docker-down"
	@echo "Reset data: make docker-clean"

docker-down: ## stop and remove all containers (preserves volumes)
	@echo "[docker] Stopping containers..."
	@docker compose down
	@echo "[docker] Stopped."

docker-logs: ## tail logs from all containers
	@docker compose logs -f

docker-jwt: ## generate JWT from running Docker container and save to .docker-jwt-token
	@echo "[docker] Generating JWT for service '$(SERVICE)'..."
	@docker exec bankie /app/bankie --mode jwt --service $(SERVICE) 2>&1 | \
		grep -oE 'eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+' | head -1 > .docker-jwt-token
	@if [ -s .docker-jwt-token ]; then \
		echo "[docker] JWT saved to .docker-jwt-token"; \
	else \
		echo "[docker] WARNING: Could not extract JWT. Is bankie container running?"; \
	fi

docker-e2e: docker-jwt ## run E2E tests against Docker stack
	@if [ ! -f .docker-jwt-token ] || [ ! -s .docker-jwt-token ]; then \
		echo "ERROR: No JWT token found. Run 'make docker-up' first."; \
		exit 1; \
	fi
	@./scripts/e2e-test.sh "$$(cat .docker-jwt-token)"

docker-interactive: ## interactive console against Docker stack (via Gateway)
	@./scripts/interactive.sh

docker-core-test: docker-jwt ## run Core API tests directly against Docker stack
	@if [ ! -f .docker-jwt-token ] || [ ! -s .docker-jwt-token ]; then \
		echo "ERROR: No JWT token found. Run 'make docker-up' first."; \
		exit 1; \
	fi
	@BASE_URL=http://localhost:3030 ./scripts/core-test.sh "$$(cat .docker-jwt-token)"

docker-clean: ## stop containers and remove volumes (full reset)
	@echo "[docker] Stopping containers and removing volumes..."
	@docker compose down -v
	@echo "[docker] Cleaned."

########
# test #
########
# cargo install cargo-nextest --locked
# cargo install cargo-llvm-cov

test: ## run tests (matches CI: single-threaded)
	SQLX_OFFLINE=true cargo test -- --test-threads=1 --nocapture

test-coverage: ## run tests with coverage report
	cargo llvm-cov nextest

##################
# overdrawn test #
##################
# Requires: running stack, k6 installed, ACCOUNT_ID env var set
# Example: ACCOUNT_ID=<uuid> make over-withdrawn-test
over-withdrawn-test:
	@TOKEN=$$(cat .docker-jwt-token 2>/dev/null || cat .local-jwt-token 2>/dev/null) && \
	k6 run -e TOKEN=$$TOKEN -e ACCOUNT_ID=$(ACCOUNT_ID) scripts/k6-overwithdraw.js

########
# lint #
########

lint: ## lints the entire codebase (matches CI exactly)
	SQLX_OFFLINE=true cargo fmt -- --check
	SQLX_OFFLINE=true cargo check --all
	SQLX_OFFLINE=true cargo clippy --all-targets --no-default-features --tests --benches -- -D warnings

ci: lint test ## run full CI pipeline locally (fmt + check + clippy + test + doc)
	SQLX_OFFLINE=true cargo doc --no-default-features --no-deps

###########
# migrate #
###########

db-pg-init-main: ## create users and passwords in postgres for your app
	@( \
	printf "Enter host for db(localhost): \n"; read -rs DB_HOST &&\
	printf "Enter pass for db: \n"; read -rs DB_PASSWORD &&\
	printf "Enter port(5432...): \n"; read -r DB_PORT &&\
	sed \
	-e "s/DB_PASSWORD/$$DB_PASSWORD/g" \
	-e "s/APP_NAME_UND/$(APP_NAME_UND)/g" \
	./db/init.sql | \
	PGPASSWORD=$$DB_PASSWORD psql -h $$DB_HOST -p $$DB_PORT -U postgres -f - \
	)

db-pg-migrate:
	@( \
	printf "Enter host for db(localhost): \n"; read -rs DB_HOST &&\
	printf "Enter pass for db: \n"; read -rs DB_PASSWORD &&\
	printf "Enter port(5432...): \n"; read -r DB_PORT &&\
	sed -i.bak \
	-e "s/DB_HOST/$$DB_HOST/g" \
	-e "s/DB_PORT/$$DB_PORT/g" \
	-e "s/DB_PASSWORD/$$DB_PASSWORD/g" \
	-e "s/APP_NAME_UND/$(APP_NAME_UND)/g" \
	./crates/bankie-core/src/repository/migrate.rs && \
	cargo run --bin migrations && \
	git stash push -m "Stash changes made by db-pg-migrate" && \
	mv ./crates/bankie-core/src/repository/migrate.rs.bak ./crates/bankie-core/src/repository/migrate.rs \
	)

#########
# build #
#########

docker-build: lint test docker-m1 ## build docker image in M1 device
	@printf "\nyou can now deploy to your env of choice:\ncd deploy\nENV=dev make deploy-latest\n"

docker-m1:
	@echo "[docker build] build local docker image on Mac M1"
	@docker build \
		-t samwang0723/$(APP_NAME):$(VERSION) \
		--build-arg LAST_MAIN_COMMIT_HASH=$(LAST_MAIN_COMMIT_HASH) \
		--build-arg LAST_MAIN_COMMIT_TIME=$(LAST_MAIN_COMMIT_TIME) \
		-f Dockerfile .

docker-amd64-deps:
	@echo "[docker buildx] install buildx depedency"
	@docker buildx create --name m1-builder
	@docker buildx use m1-builder
	@docker buildx inspect --bootstrap

docker-amd64:
	@echo "[docker buildx] build amd64 version docker image for Ubuntu AWS EC2 instance"
	@docker buildx use m1-builder
	@docker buildx build \
		--load --platform=linux/amd64 \
		-t samwang0723/$(APP_NAME):$(VERSION) \
		--build-arg LAST_MAIN_COMMIT_HASH=$(LAST_MAIN_COMMIT_HASH) \
		--build-arg LAST_MAIN_COMMIT_TIME=$(LAST_MAIN_COMMIT_TIME) \
		-f Dockerfile .

#############
# changelog #
#############

MOD_VERSION = $(shell git describe --abbrev=0 --tags `git rev-list --tags --max-count=1`)

MESSAGE_CHANGELOG_COMMIT="chore(changelog): update CHANGELOG.md for $(MOD_VERSION)"

changelog-gen: ## generates the changelog in CHANGELOG.md
	@git cliff -o ./CHANGELOG.md && \
	printf "\nchangelog generated!\n"
	git add CHANGELOG.md

changelog-commit:
	git commit -m $(MESSAGE_CHANGELOG_COMMIT) ./CHANGELOG.md

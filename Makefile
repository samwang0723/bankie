.PHONY: help test lint changelog-gen changelog-commit docker-build

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

########
# test #
########
# cargo install cargo-nextest --locked
# cargo install cargo-llvm-cov

test:
	cargo test -- --nocapture
	cargo llvm-cov nextest

##################
# overdrawn test #
##################
# Make sure to start local environment and use k6 to pressure testing
over-withdrawn-test:
	k6 run concurrent_tests/test.js

########
# lint #
########

lint: ## lints the entire codebase
	cargo clippy
	cargo fmt -- --check
	cargo check

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
	./src/repository/migrate.rs && \
	cargo run --bin migrations && \
	git stash push -m "Stash changes made by db-pg-migrate" && \
	mv ./src/repository/migrate.rs.bak ./src/repository/migrate.rs \
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

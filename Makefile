.DEFAULT_GOAL := server

.PHONY: server server-log client client-shell stress test fmt build docker-build docker-run docker-stop clean help

ADDR ?= 127.0.0.1
PORT ?= 6379
CONFIG ?= redis.conf
IDLE_TIMEOUT_SECONDS ?= 300
LOG_LEVEL ?= info

CLIENT_ADDR ?= $(ADDR):$(PORT)
CMD ?= PING

CLIENTS ?= 50
REQUESTS ?= 20000
PIPELINE ?= 10
WORKLOAD ?= advanced
KEY_SPACE ?= 1000
VALUE_SIZE ?= 64
KEY_PREFIX ?= stress

IMAGE ?= my-redis
CONTAINER ?= my-redis
DOCKER_PORT ?= 6379
DATA_DIR ?= redis-data

server:
	cargo run --bin server -- --config $(CONFIG) --addr $(ADDR) --port $(PORT) --idle-timeout-seconds $(IDLE_TIMEOUT_SECONDS)

server-log:
	cargo run --bin server -- --config $(CONFIG) --addr $(ADDR) --port $(PORT) --idle-timeout-seconds $(IDLE_TIMEOUT_SECONDS) --log --log-level $(LOG_LEVEL)

client:
	cargo run --bin client -- --addr $(CLIENT_ADDR) --cmd "$(CMD)"

client-shell:
	cargo run --bin client -- --addr $(CLIENT_ADDR)

stress:
	cargo run --bin stress -- --addr $(CLIENT_ADDR) --clients $(CLIENTS) --requests $(REQUESTS) --pipeline $(PIPELINE) --workload $(WORKLOAD) --key-space $(KEY_SPACE) --value-size $(VALUE_SIZE) --key-prefix $(KEY_PREFIX)

test:
	cargo test

fmt:
	cargo fmt

build:
	cargo build --bin server --bin client --bin stress

docker-build:
	docker build -t $(IMAGE) .

docker-run:
	docker run --rm --name $(CONTAINER) -p $(DOCKER_PORT):6379 -v $(DATA_DIR):/data $(IMAGE)

docker-stop:
	docker stop $(CONTAINER)

clean:
	cargo clean

help:
	@echo "Targets:"
	@echo "  make                         Start server"
	@echo "  make server                  Start server"
	@echo "  make server-log LOG_LEVEL=debug"
	@echo "  make client CMD=\"PING\""
	@echo "  make client-shell"
	@echo "  make stress WORKLOAD=advanced CLIENTS=50 REQUESTS=20000"
	@echo "  make test"
	@echo "  make fmt"
	@echo "  make build"
	@echo "  make docker-build IMAGE=my-redis"
	@echo "  make docker-run IMAGE=my-redis DOCKER_PORT=6379 DATA_DIR=redis-data"

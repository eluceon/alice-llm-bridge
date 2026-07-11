COMPOSE = docker compose --env-file .env --env-file docker/postgres/.env

.PHONY: up down build logs ps config

up:
	$(COMPOSE) up -d --build

down:
	$(COMPOSE) down

build:
	$(COMPOSE) build

logs:
	$(COMPOSE) logs -f

ps:
	$(COMPOSE) ps

config:
	$(COMPOSE) config

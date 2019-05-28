.PHONY: all build

all: build
	docker run -it --rm -e DSN=mysql://root:test@localhost:3306 -e SLACK_WEBHOOK_URL=https://hooks.slack.com/services --name dbpulse dbpulse

build:
	docker build -t dbpulse .

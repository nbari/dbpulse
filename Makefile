.PHONY: all build

all: build
	docker run -it --rm -e DSN=mysql://root:test@localhost:3306/dbpulse

build:
	docker build -t dbpulse .

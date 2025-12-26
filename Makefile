BINARY_NAME=vibullshit
BINARY_NAME=gouake

.PHONY: all build clean run

all: build

build:
	go build -o $(BINARY_NAME) main.go

clean:
	rm -f $(BINARY_NAME)

run: build
	./gouake

daemon: build
	./gouake --daemon

BINARY_NAME=vibullshit

.PHONY: all build clean run

all: build

build:
	go build -o $(BINARY_NAME) main.go

clean:
	rm -f $(BINARY_NAME)

run: build
	./vibullshit

daemon: build
	./vibullshit --daemon

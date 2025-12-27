BINARY_NAME=goake

.PHONY: all build clean run

all: build

build:
	go build -o dist/$(BINARY_NAME) .

clean:
	rm -f dist/$(BINARY_NAME)

run: build
	./dist/$(BINARY_NAME)

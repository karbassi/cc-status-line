.PHONY: build release clean

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean

.DEFAULT_GOAL := run

.PHONY: run
run:
	cargo run

.PHONY: test
test:
	cargo test

.PHONY: publish
publish: test
	cargo readme > README.md
	cargo publish

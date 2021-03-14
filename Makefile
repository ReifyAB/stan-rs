.DEFAULT_GOAL := run

.PHONY: run
run:
	cargo run

.PHONY: test
test:
	cargo test

.PHONY: readme
readme:
	cargo readme > README.md

.PHONY: publish
publish: test readme
	cargo publish

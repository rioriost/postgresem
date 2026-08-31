.PHONY: doctor fmt check test

doctor:
	cargo run --quiet -p postgresem -- doctor

fmt:
	cargo fmt --all --check

check:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --all-features


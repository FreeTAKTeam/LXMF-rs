.PHONY: fmt clippy test test-all test-full-targets doc deny audit udeps boundaries ci release-check api-diff licenses migration-checks forbidden-deps

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings

test:
	cargo test --workspace

test-all:
	cargo test --workspace --all-features

test-full-targets:
	cargo test --workspace --all-features --all-targets

doc:
	cargo doc --workspace --no-deps

deny:
	cargo deny check

audit:
	cargo audit

udeps:
	cargo +nightly udeps --workspace --all-targets

boundaries:
	./tools/scripts/check-boundaries.sh

forbidden-deps:
	cargo xtask forbidden-deps

ci: fmt clippy test doc boundaries migration-checks

release-check: ci deny audit

api-diff:
	@for manifest in \
		crates/libs/lxmf-core/Cargo.toml \
		crates/libs/lxmf-runtime/Cargo.toml \
		crates/libs/rns-core/Cargo.toml \
		crates/libs/rns-transport/Cargo.toml \
		crates/libs/rns-rpc/Cargo.toml; do \
		RUSTUP_TOOLCHAIN=nightly \
		RUSTC="$$(rustup which --toolchain nightly rustc)" \
		RUSTDOC="$$(rustup which --toolchain nightly rustdoc)" \
		cargo public-api --manifest-path $$manifest; \
	done

licenses:
	cargo deny check licenses

migration-checks:
	cargo xtask migration-checks

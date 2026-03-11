.PHONY: bootstrap fmt clippy test test-all test-full-targets doc deny audit udeps boundaries ci release-check api-diff licenses migration-checks forbidden-deps python-impl-bench python-impl-bench-report python-lxmd-smoke check-bin run-bin sccache-show-stats sccache-zero-stats

bootstrap:
	./tools/scripts/bootstrap-dev.sh

check-bin:
	@test -n "$(PKG)" || (echo "set PKG=<package>" >&2; exit 2)
	@test -n "$(BIN)" || (echo "set BIN=<binary>" >&2; exit 2)
	cargo check -p $(PKG) --bin $(BIN)

run-bin:
	@test -n "$(PKG)" || (echo "set PKG=<package>" >&2; exit 2)
	@test -n "$(BIN)" || (echo "set BIN=<binary>" >&2; exit 2)
	cargo run -p $(PKG) --bin $(BIN) -- $(ARGS)

sccache-show-stats:
	@if command -v sccache >/dev/null 2>&1; then sccache --show-stats; else echo "sccache not installed"; fi

sccache-zero-stats:
	@if command -v sccache >/dev/null 2>&1; then sccache --zero-stats; else echo "sccache not installed"; fi

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
		crates/libs/lxmf-sdk/Cargo.toml \
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

python-impl-bench:
	cargo xtask python-impl-bench-compare

python-impl-bench-report:
	cargo xtask python-impl-bench-report

python-lxmd-smoke:
	./tools/scripts/python-lxmd-rust-lxmd-smoke.sh

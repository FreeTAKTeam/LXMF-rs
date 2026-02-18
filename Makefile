RETICULUM_PY_PATH ?= ../Reticulum
RETICULUM_PY_ABS := $(abspath $(RETICULUM_PY_PATH))
SIDEBAND_PATH ?= ../Sideband
SIDEBAND_ABS := $(abspath $(SIDEBAND_PATH))
RUN_SIDEBAND_E2E ?= 0
RUN_INTEROP_GATES ?= 1

.PHONY: all clean remove_symlinks create_symlinks build_wheel build_sdist build_spkg release upload interop-gate soak-rnx sideband-e2e release-gate-local production-gate-local coverage test test-fast test-all test-all-targets test-full test-full-targets

CORE_TESTS = api_surface error_smoke smoke lxmf_cli_args lxmf_daemon_commands lxmf_daemon_supervisor \
	lxmf_iface_commands lxmf_message_commands lxmf_peer_commands lxmf_profile lxmf_rpc_client lxmf_runtime_context
CORE_TEST_ARGS = $(CORE_TESTS:%=--test %)

all: release

clean:
	@echo Cleaning...
	-rm -r ./build
	-rm -r ./dist

remove_symlinks:
	@echo Removing symlinks for build...
	-rm ./RNS
	-rm ./LXMF/Utilities/LXMF

create_symlinks:
	@echo Creating symlinks...
	-ln -s ../Reticulum/RNS ./
	-ln -s ../../LXMF ./LXMF/Utilities/LXMF

build_wheel:
	python3 setup.py bdist_wheel

build_sdist:
	python3 setup.py sdist

build_spkg: remove_symlinks build_sdist create_symlinks

release: remove_symlinks build_wheel create_symlinks

upload:
	@echo Uploading to PyPi...
	twine upload dist/*

test:
	# Fast local compatibility pass: core behavior tests + CLI surface.
	cargo test --workspace --features cli --lib
	cargo test --workspace --features cli $(CORE_TEST_ARGS)

test-all:
	# Full feature matrix for release/compatibility confidence.
	cargo test --workspace --all-features

test-fast:
	# Alias for explicit fast core test pass.
	$(MAKE) test

test-all-targets:
	# Fast full-target sweep on the CLI feature set.
	cargo test --workspace --all-targets --features cli

test-full-targets:
	# Maximum coverage run (all features + all targets).
	cargo test --workspace --all-features --all-targets

test-full: test-full-targets

interop-gate:
	@if [ ! -d "$(RETICULUM_PY_ABS)/RNS" ]; then \
		echo "RETICULUM_PY_PATH must point to the Python Reticulum repo (missing $(RETICULUM_PY_ABS)/RNS)"; \
		exit 1; \
	fi
	LXMF_PYTHON_INTEROP=1 \
	PYTHONPATH="$(CURDIR):$(RETICULUM_PY_ABS):$$PYTHONPATH" \
	cargo test -p lxmf --test python_interop_gate --features cli -- --nocapture
	LXMF_PYTHON_INTEROP=1 \
	PYTHONPATH="$(CURDIR):$(RETICULUM_PY_ABS):$$PYTHONPATH" \
	cargo test -p lxmf --test python_client_interop_gate --features cli -- --nocapture
	LXMF_PYTHON_INTEROP=1 \
	PYTHONPATH="$(CURDIR):$(RETICULUM_PY_ABS):$$PYTHONPATH" \
	cargo test -p lxmf --test python_client_replay_gate --features cli -- --nocapture

soak-rnx:
	./scripts/soak-rnx.sh

sideband-e2e:
	@if [ ! -d "$(RETICULUM_PY_ABS)/RNS" ]; then \
		echo "RETICULUM_PY_PATH must point to the Python Reticulum repo (missing $(RETICULUM_PY_ABS)/RNS)"; \
		exit 1; \
	fi
	@if [ ! -d "$(SIDEBAND_ABS)/sbapp" ]; then \
		echo "SIDEBAND_PATH must point to the Sideband repo (missing $(SIDEBAND_ABS)/sbapp)"; \
		exit 1; \
	fi
	./scripts/sideband-e2e.py \
		--sideband-path "$(SIDEBAND_ABS)" \
		--reticulum-py-path "$(RETICULUM_PY_ABS)"

release-gate-local:
	make test-all
	@if [ "$(RUN_INTEROP_GATES)" = "1" ]; then \
		make interop-gate RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)"; \
	else \
		echo "Skipping interop gate (set RUN_INTEROP_GATES=1 to run)"; \
	fi
	cargo run --manifest-path crates/reticulum/Cargo.toml --features cli-tools --bin rnx -- e2e --timeout-secs 20
	@if [ "$(RUN_SIDEBAND_E2E)" = "1" ]; then \
		make sideband-e2e RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)" SIDEBAND_PATH="$(SIDEBAND_PATH)"; \
	fi

production-gate-local:
	make release-gate-local RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)" SIDEBAND_PATH="$(SIDEBAND_PATH)" RUN_SIDEBAND_E2E=1

coverage:
	@command -v cargo-llvm-cov >/dev/null || cargo install cargo-llvm-cov --locked
	rustup component add llvm-tools-preview
	cargo llvm-cov --workspace --all-targets --lcov --output-path coverage.lcov

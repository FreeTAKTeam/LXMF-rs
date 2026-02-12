RETICULUM_PY_PATH ?= ../Reticulum
RETICULUM_PY_ABS := $(abspath $(RETICULUM_PY_PATH))
SIDEBAND_PATH ?= ../Sideband
SIDEBAND_ABS := $(abspath $(SIDEBAND_PATH))
RUN_SIDEBAND_E2E ?= 0

.PHONY: all clean remove_symlinks create_symlinks build_wheel build_sdist build_spkg release upload interop-gate soak-rnx sideband-e2e release-gate-local production-gate-local coverage

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
		--reticulum-rs-path ../Reticulum-rs \
		--sideband-path "$(SIDEBAND_ABS)" \
		--reticulum-py-path "$(RETICULUM_PY_ABS)"

release-gate-local:
	cargo test --workspace --all-targets --all-features
	make interop-gate RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)"
	cargo run --manifest-path ../Reticulum-rs/crates/reticulum/Cargo.toml --features cli-tools --bin rnx -- e2e --timeout-secs 20
	@if [ "$(RUN_SIDEBAND_E2E)" = "1" ]; then \
		make sideband-e2e RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)" SIDEBAND_PATH="$(SIDEBAND_PATH)"; \
	fi

production-gate-local:
	make release-gate-local RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)" SIDEBAND_PATH="$(SIDEBAND_PATH)" RUN_SIDEBAND_E2E=1

coverage:
	@command -v cargo-llvm-cov >/dev/null || cargo install cargo-llvm-cov --locked
	rustup component add llvm-tools-preview
	cargo llvm-cov --workspace --all-targets --lcov --output-path coverage.lcov

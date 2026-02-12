RETICULUM_PY_PATH ?= ../reticulum
RETICULUM_PY_ABS := $(abspath $(RETICULUM_PY_PATH))

.PHONY: all clean remove_symlinks create_symlinks build_wheel build_sdist build_spkg release upload interop-gate soak-rnx release-gate-local coverage

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

release-gate-local:
	cargo test --workspace --all-targets --all-features
	make interop-gate RETICULUM_PY_PATH="$(RETICULUM_PY_PATH)"
	cargo run --manifest-path ../Reticulum-rs/crates/reticulum/Cargo.toml --features cli-tools --bin rnx -- e2e --timeout-secs 20

coverage:
	@command -v cargo-llvm-cov >/dev/null || cargo install cargo-llvm-cov --locked
	rustup component add llvm-tools-preview
	cargo llvm-cov --workspace --all-targets --lcov --output-path coverage.lcov

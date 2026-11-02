# debug mode rapide > opti
build:
		cargo build --verbose
# release mode rapide < opti
release:
		RUSTFLAGS="-C codegen-units=1 -C code-model=kernel" cargo build --release --verbose
# nettoyage
clean:
		cargo clean
build:
	@npm install
	@npm run build
	@awk 1 build/static/js/*.js > public/script.js

dev: src/result.test.json
	@npm start

src/result.test.json:
	@cd ../../../../ && cargo +nightly run --release --bin cargo-compliance -- report --spec-pattern 'specs/**/*.toml' --source-pattern 'quic/**/*.rs' --workspace --exclude compliance --exclude cargo-compliance --require-tests false --blob-link https://github.com/awslabs/s2n-quic/blob/main/ --issue-link https://github.com/awslabs/s2n-quic/issues/ --json common/compliance/cargo-compliance/www/src/result.test.json --no-cargo

.PHONY: build dev

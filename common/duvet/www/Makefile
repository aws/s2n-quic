build:
	@npm install
	@npm run build
	@awk 1 build/static/js/*.js > public/script.js

dev: src/result.test.json
	@npm start

src/result.test.json:
	@cd ../../../ && cargo run --release --bin duvet -- report --spec-pattern 'specs/**/*.toml' --source-pattern 'quic/**/*.rs' --workspace --exclude duvet --require-tests false --blob-link https://github.com/aws/s2n-quic/blob/main/ --issue-link https://github.com/aws/s2n-quic/issues/ --json common/duvet/www/src/result.test.json --no-cargo

.PHONY: build dev

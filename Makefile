lint:
	cargo fmt --all
	cargo clippy --fix --allow-dirty --all-targets --all-features -- --deny warnings

perf:
	cargo bench --bench tm_performance -- --sample-count 20 --sample-size 10 --format terse

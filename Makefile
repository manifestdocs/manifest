.PHONY: run run-local run-cloud build test check clean

# Default: run in local mode
run: run-local

# Run in local mode (no authentication)
run-local:
	MANIFEST_MODE=local cargo run

# Run in cloud mode (requires Clerk config)
# Set CLERK_DOMAIN and CLERK_AUTHORIZED_PARTIES in .env or export them
run-cloud:
	@if [ -z "$$CLERK_DOMAIN" ]; then \
		echo "Error: CLERK_DOMAIN not set"; \
		echo "Export it or create a .env file with:"; \
		echo "  CLERK_DOMAIN=your-domain.clerk.accounts.dev"; \
		echo "  CLERK_AUTHORIZED_PARTIES=http://localhost:5173"; \
		exit 1; \
	fi
	MANIFEST_MODE=cloud cargo run

# Run cloud mode with .env file
run-cloud-env:
	@if [ -f .env ]; then \
		export $$(grep -v '^#' .env | xargs) && \
		MANIFEST_MODE=cloud cargo run; \
	else \
		echo "Error: .env file not found"; \
		echo "Create one with CLERK_DOMAIN and CLERK_AUTHORIZED_PARTIES"; \
		exit 1; \
	fi

# Build release binary
build:
	cargo build --release

# Run all tests
test:
	cargo test

# Type check without building
check:
	cargo check

# Clean build artifacts
clean:
	cargo clean

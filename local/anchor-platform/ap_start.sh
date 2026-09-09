#!/bin/bash
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}Starting Anchor Platform setup for anchor-rust...${NC}"

if ! command -v docker-compose &> /dev/null && ! command -v docker compose &> /dev/null; then
    echo -e "${RED}Error: docker-compose is not installed.${NC}"
    exit 1
fi
if command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE="docker-compose"
else
    DOCKER_COMPOSE="docker compose"
fi
if ! command -v stellar &> /dev/null; then
    echo -e "${RED}Error: Stellar CLI is not installed.${NC}"
    echo "Install it from: https://github.com/stellar/stellar-cli"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANCHOR_RUST_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$SCRIPT_DIR"

echo -e "${YELLOW}Step 1: Reading anchor-rust's own distribution account from .env...${NC}"
DISTRIBUTION_SEED=$(grep -E '^DISTRIBUTION_SEED=' "$ANCHOR_RUST_ROOT/.env" | cut -d= -f2-)
if [ -z "$DISTRIBUTION_SEED" ]; then
    echo -e "${RED}Error: DISTRIBUTION_SEED not found in $ANCHOR_RUST_ROOT/.env${NC}"
    exit 1
fi
DISTRIBUTION_ACCOUNT=$(cd "$ANCHOR_RUST_ROOT" && DISTRIBUTION_SEED="$DISTRIBUTION_SEED" cargo run --quiet --example print_distribution_pubkey)
echo "  Distribution account (custody stays in anchor-rust): $DISTRIBUTION_ACCOUNT"

echo -e "${YELLOW}Step 2: Checking for the platform's own SEP-10 signing keypair...${NC}"
KEYPAIR_NAME="ar-ap-sep10-account"
if stellar keys secret "$KEYPAIR_NAME" &>/dev/null; then
    echo "  Found existing keypair: $KEYPAIR_NAME"
else
    echo "  Generating and funding a new one..."
    KEYPAIR_OUTPUT=$(stellar keys generate "$KEYPAIR_NAME" --fund --network testnet 2>&1 || stellar keys generate "$KEYPAIR_NAME" --fund 2>&1 || true)
    if ! echo "$KEYPAIR_OUTPUT" | grep -q "Key saved"; then
        echo -e "${RED}Error: Failed to generate SEP-10 signing keypair${NC}"
        echo "$KEYPAIR_OUTPUT"
        exit 1
    fi
fi
HOST_SEP10_SECRET_KEY=$(stellar keys secret "$KEYPAIR_NAME" 2>&1 | head -1)
HOST_SEP10_ACCOUNT=$(stellar keys public-key "$KEYPAIR_NAME" 2>&1 | head -1)

echo -e "${GREEN}Platform SEP-10 signing account:${NC} $HOST_SEP10_ACCOUNT"
echo -e "${GREEN}Distribution account (owned by anchor-rust):${NC} $DISTRIBUTION_ACCOUNT"

echo -e "${YELLOW}Step 3: Templating config files...${NC}"
export HOST_SEP10_SECRET_KEY
sed "s|\${DISTRIBUTION_ACCOUNT}|$DISTRIBUTION_ACCOUNT|g" config/assets.yaml.template > config/assets.yaml
sed "s|\${DISTRIBUTION_ACCOUNT}|$DISTRIBUTION_ACCOUNT|g; s|\${HOST_SEP10_ACCOUNT}|$HOST_SEP10_ACCOUNT|g" \
    config/stellar.localhost.toml.template > config/stellar.localhost.toml
echo "  wrote config/assets.yaml and config/stellar.localhost.toml"

echo -e "${YELLOW}Step 4: Starting docker-compose (platform + kafka + platform-db)...${NC}"
HOST_SEP10_SECRET_KEY="$HOST_SEP10_SECRET_KEY" $DOCKER_COMPOSE up -d

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Anchor Platform is starting up.${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Now run anchor-rust itself on the host (separate terminal):"
echo "  cd $ANCHOR_RUST_ROOT && cargo run"
echo ""
echo "Services:"
echo "  SEP server:    http://localhost:8080"
echo "  Platform API:  http://localhost:8085"
echo "  anchor-rust:   http://localhost:8091 (once you run it)"
echo ""
echo "To view logs: $DOCKER_COMPOSE logs -f"
echo "To stop:      $DOCKER_COMPOSE down"

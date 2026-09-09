CREATE TABLE customers (
    id UUID PRIMARY KEY,
    account TEXT NOT NULL,
    memo TEXT,
    memo_type TEXT,
    customer_type TEXT NOT NULL,
    owner_account TEXT NOT NULL,
    owner_memo TEXT,
    fields JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX customers_account_idx ON customers (account, memo, customer_type);

CREATE TABLE sep24_transactions (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    account TEXT NOT NULL,
    memo TEXT,
    memo_type TEXT,
    asset_code TEXT NOT NULL,
    amount_in NUMERIC,
    amount_out NUMERIC,
    amount_fee NUMERIC,
    stellar_transaction_id TEXT,
    message TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE TABLE sep31_transactions (
    id UUID PRIMARY KEY,
    status TEXT NOT NULL,
    creator_account TEXT NOT NULL,
    creator_memo TEXT,
    asset_code TEXT NOT NULL,
    amount_in NUMERIC NOT NULL,
    amount_out NUMERIC,
    fee NUMERIC,
    sender_id TEXT,
    receiver_id TEXT,
    stellar_memo TEXT NOT NULL,
    stellar_memo_type TEXT NOT NULL,
    required_info_message TEXT,
    stellar_transaction_id TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

ALTER TABLE sep24_transactions ADD COLUMN platform_transaction_id TEXT;
CREATE UNIQUE INDEX sep24_transactions_platform_id_idx ON sep24_transactions (platform_transaction_id)
    WHERE platform_transaction_id IS NOT NULL;

ALTER TABLE sep31_transactions ADD COLUMN platform_transaction_id TEXT;
CREATE UNIQUE INDEX sep31_transactions_platform_id_idx ON sep31_transactions (platform_transaction_id)
    WHERE platform_transaction_id IS NOT NULL;

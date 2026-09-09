ALTER TABLE sep24_transactions ADD COLUMN withdraw_memo TEXT;
CREATE INDEX sep24_transactions_withdraw_memo_idx ON sep24_transactions (withdraw_memo);

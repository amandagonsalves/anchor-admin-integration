//! SEP-31's wire protocol (`/sep31/info`, `POST/GET/PATCH /transactions`) is
//! served entirely by the anchor platform now. What's left here is our own
//! settlement/kyc-gating logic (`worker`), driven by the `transaction_created`
//! event (see `event::handle_sep31_created`) and by the SEP-12 `PUT /customer`
//! callback resuming a KYC-gated transaction once fields are complete.

pub mod worker;

//! SEP-24's wire protocol (`/sep24/info`, `/sep24/transactions/...`) is
//! served entirely by the anchor platform now. What's left here is our own
//! side of the flow: the hosted interactive UI (`interactive`) and the
//! custody/settlement logic the platform's `transaction_created` /
//! Stellar Observer events drive forward (`worker`).

pub mod interactive;
pub mod worker;

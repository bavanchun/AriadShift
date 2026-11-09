//! Email document readers for EML and Outlook MSG formats.

pub mod eml;
pub mod msg;

pub use eml::read as read_eml;
pub use msg::read as read_msg;

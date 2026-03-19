use std::sync::atomic::{AtomicI64, Ordering};

static NEXT_MESSAGE_ID: AtomicI64 = AtomicI64::new(1);
static NEXT_PART_ID: AtomicI64 = AtomicI64::new(1);

pub(crate) fn new_message_id() -> String {
    NEXT_MESSAGE_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

pub(crate) fn new_part_id() -> String {
    NEXT_PART_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

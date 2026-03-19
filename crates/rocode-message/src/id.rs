pub(crate) fn new_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn new_part_id() -> String {
    format!("prt_{}", uuid::Uuid::new_v4().simple())
}

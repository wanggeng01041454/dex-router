use std::cell::RefCell;

thread_local! {
    static TRACE_ID: RefCell<Option<String>> = RefCell::new(None);
}

pub fn set_trace_id(id: Option<String>) {
    TRACE_ID.with(|cell| {
        *cell.borrow_mut() = id;
    });
}

pub fn get_trace_id() -> Option<String> {
    TRACE_ID.with(|cell| cell.borrow().clone())
}
use crate::storage::messages::MessageRecord;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type InboundHandler = Rc<dyn Fn(&MessageRecord) -> bool>;
type BridgeMap = HashMap<String, InboundHandler>;

thread_local! {
    static BRIDGE: RefCell<BridgeMap> = RefCell::new(HashMap::new());
}

pub fn reset() {
    BRIDGE.with(|bridge| bridge.borrow_mut().clear());
}

pub fn register(identity: impl Into<String>, on_inbound: InboundHandler) {
    BRIDGE.with(|bridge| {
        bridge.borrow_mut().insert(identity.into(), on_inbound);
    });
}

pub fn deliver_outbound(record: &MessageRecord) -> bool {
    BRIDGE.with(|bridge| {
        bridge.borrow().get(&record.destination).is_some_and(|handler| handler(record))
    })
}

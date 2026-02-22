#![cfg(feature = "loom-tests")]

use loom::sync::{Arc, Mutex};
use loom::thread;

#[test]
fn loom_terminal_transition_is_single_winner() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(0_u8));

        let stop_state = Arc::clone(&state);
        let stop = thread::spawn(move || {
            let mut guard = stop_state.lock().expect("state mutex poisoned");
            if *guard == 0 {
                *guard = 1;
            }
        });

        let fail_state = Arc::clone(&state);
        let fail = thread::spawn(move || {
            let mut guard = fail_state.lock().expect("state mutex poisoned");
            if *guard == 0 {
                *guard = 2;
            }
        });

        stop.join().expect("stop join");
        fail.join().expect("fail join");

        let terminal = *state.lock().expect("state mutex poisoned");
        assert!(terminal == 1 || terminal == 2, "exactly one terminal transition should win");
    });
}

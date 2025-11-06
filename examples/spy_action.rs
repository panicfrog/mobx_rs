//! Demonstrates how the core MobX-inspired APIs interact together.
//!
//! Run with `cargo run --example spy_action`.

use mobx_rs::observable::value::ObservableValue;
use mobx_rs::spy::{SpyEvent, register};
use mobx_rs::{
    ActionPolicy, Computed, ComputedOptions, action, autorun, drain_strict_mode_warnings,
    run_pending_reactions, set_enforce_actions,
};

fn main() {
    // Subscribe to spy diagnostics so we can see the runtime events.
    let mut subscription = register(|event: &SpyEvent| {
        println!("[spy] {event:?}");
    });

    let counter = ObservableValue::new("counter", 0);
    let doubled = {
        let counter = counter.clone();
        Computed::new(ComputedOptions::new(move || counter.get() * 2).name("doubled"))
    };

    let handle = autorun({
        let doubled = doubled.clone();
        move || println!("[autorun] doubled = {}", doubled.get())
    });

    set_enforce_actions(ActionPolicy::Always);

    action("increment", || counter.update(|value| *value += 1));
    action("set_to_five", || counter.set(5));
    run_pending_reactions();

    set_enforce_actions(ActionPolicy::Never);
    handle.dispose();
    subscription.dispose();

    let warnings = drain_strict_mode_warnings();
    if !warnings.is_empty() {
        println!("strict mode warnings: {warnings:?}");
    }
}

use mobx_rs::{
    ActionPolicy, action, allow_state_changes, drain_strict_mode_warnings, enforce_actions_policy,
    run_in_action, set_enforce_actions,
};

#[test]
fn test_nested_actions_return_result() {
    set_enforce_actions(ActionPolicy::Always);
    drain_strict_mode_warnings();

    let value = action("outer", || run_in_action(|| 42));
    assert_eq!(value, 42);
    assert!(drain_strict_mode_warnings().is_empty());

    set_enforce_actions(ActionPolicy::Never);
}

#[test]
fn test_allow_state_changes_restores_policy() {
    set_enforce_actions(ActionPolicy::Always);
    drain_strict_mode_warnings();

    let result = allow_state_changes(|| "ok");
    assert_eq!(result, "ok");
    assert_eq!(enforce_actions_policy(), ActionPolicy::Always);
    assert!(drain_strict_mode_warnings().is_empty());

    set_enforce_actions(ActionPolicy::Never);
}

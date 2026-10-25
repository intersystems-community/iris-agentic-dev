//! `testing::EnvGuard`: sets and unsets process variables for one test and puts every one back when
//! it is dropped, on a panic too (spec 141 T186).
//!
//! These tests read the environment with `std::env::var` and change it only through the guard, so
//! they follow the rule they test.

use iris_agentic_dev_core::testing::EnvGuard;

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

#[test]
fn set_restores_an_unset_variable_to_unset() {
    let key = "IAD_TEST_ENVGUARD_A";
    {
        let mut g = EnvGuard::new();
        g.unset(key);
        assert_eq!(var(key), None);
    }
    {
        let mut g = EnvGuard::new();
        g.set(key, "during");
        assert_eq!(var(key).as_deref(), Some("during"));
    }
    let _g = EnvGuard::new();
    assert_eq!(var(key), None, "a variable that was unset stays unset");
}

#[test]
fn set_restores_the_previous_value() {
    let key = "IAD_TEST_ENVGUARD_B";
    let mut outer = EnvGuard::new();
    outer.set(key, "before");
    {
        let mut inner = EnvGuard::new();
        inner.set(key, "during");
        assert_eq!(var(key).as_deref(), Some("during"));
    }
    assert_eq!(
        var(key).as_deref(),
        Some("before"),
        "dropping the inner guard puts the outer value back"
    );
}

#[test]
fn unset_restores_a_variable_that_was_set() {
    let key = "IAD_TEST_ENVGUARD_C";
    let mut outer = EnvGuard::new();
    outer.set(key, "kept");
    {
        let mut inner = EnvGuard::new();
        inner.unset(key);
        assert_eq!(var(key), None);
    }
    assert_eq!(var(key).as_deref(), Some("kept"));
}

#[test]
fn a_key_touched_twice_restores_the_value_from_before_the_first_touch() {
    let key = "IAD_TEST_ENVGUARD_D";
    let mut outer = EnvGuard::new();
    outer.set(key, "original");
    {
        let mut inner = EnvGuard::new();
        inner
            .set(key, "one")
            .set(key, "two")
            .unset(key)
            .set(key, "three");
        assert_eq!(var(key).as_deref(), Some("three"));
    }
    assert_eq!(var(key).as_deref(), Some("original"));
}

#[test]
fn the_value_comes_back_after_a_panic() {
    let key = "IAD_TEST_ENVGUARD_E";
    let mut outer = EnvGuard::new();
    outer.set(key, "safe");
    let result = std::panic::catch_unwind(|| {
        let mut g = EnvGuard::new();
        g.set(key, "leaked?");
        panic!("the test failed halfway");
    });
    assert!(result.is_err(), "the closure must have panicked");
    assert_eq!(var(key).as_deref(), Some("safe"));
}

#[test]
fn guards_nest_on_one_thread_without_deadlock() {
    let _a = EnvGuard::new();
    let _b = EnvGuard::new();
    let mut c = EnvGuard::new();
    c.set("IAD_TEST_ENVGUARD_F", "x");
    assert_eq!(var("IAD_TEST_ENVGUARD_F").as_deref(), Some("x"));
}

#[test]
fn clean_removes_every_behavior_variable_and_restores_them() {
    let mut outer = EnvGuard::new();
    outer.set("IRIS_CONTAINER", "outer-container");
    outer.set("OBJECTSCRIPT_WORKSPACE", "/outer/workspace");
    {
        let _clean = EnvGuard::clean();
        for var_name in iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS
            .iter()
            .filter(|v| **v != "IAD_NO_KEYCHAIN")
        {
            assert_eq!(
                var(var_name),
                None,
                "{var_name} must be unset in a clean guard"
            );
        }
    }
    assert_eq!(var("IRIS_CONTAINER").as_deref(), Some("outer-container"));
    assert_eq!(
        var("OBJECTSCRIPT_WORKSPACE").as_deref(),
        Some("/outer/workspace")
    );
}

#[test]
fn two_threads_do_not_overlap_while_guards_are_held() {
    // The lock is process-wide: while one thread holds a guard, another's `new` waits. If it did
    // not, both threads would see the other's value in the middle of their own test.
    let key = "IAD_TEST_ENVGUARD_G";
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|mine| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut g = EnvGuard::new();
                g.set(key, mine);
                for _ in 0..200 {
                    assert_eq!(var(key).as_deref(), Some(mine), "another guard overlapped");
                    std::thread::yield_now();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread must not see the other's value");
    }
}

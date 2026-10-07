#[test] fn before_helper() { assert!(test_only_helper()); } pub fn positive(value: i32) -> bool {
    threshold(value)
}

fn threshold(value: i32) -> bool {
    value > 0
}

#[cfg(test)]
fn test_only_helper() -> bool { positive(1) }

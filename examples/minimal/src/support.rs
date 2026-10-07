#[test] fn before_helper() { assert!(positive(1)); } pub fn positive(value: i32) -> bool {
    threshold(value)
}

fn threshold(value: i32) -> bool {
    value > 0
}

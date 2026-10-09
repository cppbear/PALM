pub mod shared;

pub fn helper(value: i32) -> i32 {
    value + 10
}

pub fn lib_only(value: i32) -> i32 {
    shared::classify(value)
}

pub struct Subject {
    pub value: i32,
}

mod shared;

fn helper(value: i32) -> i32 {
    value - 10
}

fn bin_only(value: i32) -> i32 {
    shared::classify(value) + mixed_fixture::helper(value)
}

fn library_type(subject: mixed_fixture::Subject) -> i32 {
    subject.value
}

fn main() {
    panic!("the unit-test pipeline must not execute main");
}

mod support;

#[cfg(test)]
mod tests {
    #[test]
    fn existing_unit_test() { assert_eq!(super::classify(2), 1); }
}

pub struct Gauge {
    pub value: i32,
}

impl Gauge {
    pub fn label(&self) -> i32 {
        classify(self.value)
    }
}

pub fn classify(value: i32) -> i32 {
    if support::positive(value) {
        1
    } else {
        0
    }
}

pub mod nested {
    pub fn double(value: i32) -> i32 {
        value * 2
    }
}

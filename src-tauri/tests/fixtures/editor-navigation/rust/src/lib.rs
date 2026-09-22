pub fn target() {}

pub fn caller_a() {
    target();
}

pub fn caller_b() {
    target();
}

mod other {
    pub fn target() {}
}

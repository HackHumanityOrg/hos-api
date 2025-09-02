pub fn bool_i8(b: bool) -> i8 {
    if b {
        1
    } else {
        0
    }
}

pub fn opt_bool_i8(v: Option<bool>, default: i8) -> i8 {
    v.map(bool_i8).unwrap_or(default)
}

pub fn opt_bool_to_i8(v: Option<bool>) -> Option<i8> {
    v.map(bool_i8)
}

pub fn opt_string(s: Option<String>) -> String {
    s.unwrap_or_default()
}

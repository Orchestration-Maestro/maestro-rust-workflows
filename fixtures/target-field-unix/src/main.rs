//! A field-read fixture for native target checks.

struct Observation {
    value: u8,
}

fn main() {
    let observation = Observation { value: 7 };
    #[cfg(unix)]
    assert_eq!(observation.value, 7);
    let _ = observation;
}

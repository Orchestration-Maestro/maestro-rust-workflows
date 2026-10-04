pub fn double(value:u32)->u32 {value*2}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles_value() {
        assert_eq!(double(2), 4);
    }
}

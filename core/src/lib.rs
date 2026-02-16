pub fn core_ok() -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_smoke() {
        assert!(core_ok());
    }
}

pub fn skillrunner_ok() -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skillrunner_smoke() {
        assert!(skillrunner_ok());
    }
}

pub fn check_for_insufficient_funds(err_str: &str) -> bool {
    err_str.contains("insufficient funds") || err_str.contains("insufficient allowance")
}

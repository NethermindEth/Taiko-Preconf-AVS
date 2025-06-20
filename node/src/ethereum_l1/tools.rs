pub fn check_for_insufficient_funds(err_str: &str) -> bool {
    err_str.contains("insufficient funds") || err_str.contains("insufficient allowance")
}

/// 0x46afbf54 -> AnchorBlockIdTooSmall()
/// 0x1999aed2 -> TimestampTooSmall()
/// 0xfe1698b2 -> AnchorBlockIdSmallerThanParent()
/// 0x21389b84 -> TimestampSmallerThanParent()
pub fn check_for_reanchor_required(err_str: &str) -> bool {
    err_str.contains("0x46afbf54")
        || err_str.contains("0x1999aed2")
        || err_str.contains("0xfe1698b2")
        || err_str.contains("0x21389b84")
}

/// 0x3d32ffdb -> TimestampTooLarge()
/// 0x2b44f010 -> ZeroAnchorBlockHash()
pub fn check_for_too_early_estimation(err_str: &str) -> bool {
    err_str.contains("0x3d32ffdb") || err_str.contains("0x2b44f010")
}

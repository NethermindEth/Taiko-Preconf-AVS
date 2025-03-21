use anyhow::Error;

pub fn read_jwt_secret(file_path: &str) -> Result<[u8; 32], Error> {
    let secret = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read JWT secret from file: {}", e))?;
    let secret_bytes = hex::decode(secret.strip_prefix("0x").unwrap_or(&secret))
        .map_err(|e| anyhow::anyhow!("Failed to decode hex string: {}", e))?;
    let secret_bytes: [u8; 32] = secret_bytes
        .try_into()
        .map_err(|e| anyhow::anyhow!("Failed to convert secret bytes to [u8; 32]: {:?}", e))?;
    Ok(secret_bytes)
}

use anyhow::Error;

pub fn read_jwt_secret(file_path: &str) -> Result<String, Error> {
    let secret = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read JWT secret: {}", e))?;
    Ok(secret)
}

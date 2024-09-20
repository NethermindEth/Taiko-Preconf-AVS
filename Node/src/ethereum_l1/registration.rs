use anyhow::Error;

const PRECONF_NOT_REGISTERED: &[u8] = b"PreconferNotRegistered()";
const VALIDATOR_ALREADY_ACTIVE: &[u8] = b"ValidatorAlreadyActive()";

// the error is e.g. "Reverted ;l_$"
// second part is first 4 bytes of keccak encoded
// error string
pub fn decode_add_validator_error(error: &str) -> Result<String, Error> {
    let error_bytes = error.as_bytes();
    if error_bytes.len() < 13 {
        return Err(anyhow::anyhow!(
            "Error string is too short for addValidator error"
        ));
    }
    let error_selector = &error_bytes[error_bytes.len() - 4..];

    let preconf_not_registered_selector: [u8; 4] = first_4_bytes_of_keccak(PRECONF_NOT_REGISTERED)?;
    let validator_already_active_selector: [u8; 4] =
        first_4_bytes_of_keccak(VALIDATOR_ALREADY_ACTIVE)?;

    if error_selector == preconf_not_registered_selector {
        return Ok("Preconfer not registered".to_string());
    }
    if error_selector == validator_already_active_selector {
        return Ok("Validator already active".to_string());
    }
    Err(anyhow::anyhow!(
        "decode_add_validator_error: Unknown error: {}",
        error
    ))
}

fn first_4_bytes_of_keccak(input: &[u8]) -> Result<[u8; 4], Error> {
    Ok(crate::utils::bytes_tools::hash_bytes_with_keccak(input)[..4].try_into()?)
}

const PRECONF_ALREADY_REGISTERED: &[u8] = b"PreconferAlreadyRegistered()";

pub fn decode_register_preconfer_error(error: &str) -> Result<String, Error> {
    let error_bytes = error.as_bytes();
    if error_bytes.len() < 13 {
        return Err(anyhow::anyhow!(
            "Error string is too short for addValidator error"
        ));
    }
    let error_selector = &error_bytes[error_bytes.len() - 4..];
    let preconf_already_registered_selector: [u8; 4] =
        first_4_bytes_of_keccak(PRECONF_ALREADY_REGISTERED)?;

    if error_selector == preconf_already_registered_selector {
        return Ok("Preconfer already registered".to_string());
    }

    Err(anyhow::anyhow!(
        "decode_register_preconfer_error: Unknown error: {}",
        error
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_add_validator_error() {
        let preconf_not_registered_error = "Reverted ;l_$";
        let result = decode_add_validator_error(&preconf_not_registered_error);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Preconfer not registered");
    }

    #[test]
    fn test_check_register_preconfer_error() {
        let preconf_already_registered_error = "Reverted þaM";
        let result = decode_register_preconfer_error(&preconf_already_registered_error);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Preconfer already registered");
    }
}

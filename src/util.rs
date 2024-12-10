use std::io::{self, Error, ErrorKind};

use base64::{
    alphabet::URL_SAFE,
    engine::{general_purpose::NO_PAD, GeneralPurpose},
    Engine,
};

pub fn decode_base64_non_strict(input: &str) -> io::Result<String> {
    let mut to_url = String::new();

    if !input.is_ascii() {
        return Err(Error::new(ErrorKind::InvalidInput, "Invalid URL encoding"));
    }

    // check if the input contains any of: '+', '/', or '='
    if input.contains('+') || input.contains('/') || input.contains('=') {
        let padding_index = input.find('=').unwrap_or(input.len());

        input[..padding_index].clone_into(&mut to_url);

        // SAFETY: We're certain that the input is ASCII (0-0x7F)
        let bytes = unsafe { to_url.as_bytes_mut() };

        for b in bytes.iter_mut() {
            if *b == b'+' {
                *b = b'-';
            } else if *b == b'/' {
                *b = b'_';
            }
        }
    }

    let base64_url_data = if to_url.is_empty() { input } else { &to_url };

    let decoded = GeneralPurpose::new(&URL_SAFE, NO_PAD)
        .decode(base64_url_data)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "Invalid URL encoding"))?;

    String::from_utf8(decoded)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "Encoded URL is not valid UTF-8"))
}

pub(super) const LOGIN: &[&str] = &[
    "codex",
    "login",
    "-c",
    "cli_auth_credentials_store=file",
    "--device-auth",
];

pub(super) struct Decoder {
    parser: vt100::Parser,
}

pub(super) struct Prompt {
    pub url: url::Url,
    pub code: String,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            parser: vt100::Parser::new(40, 240, 0),
        }
    }

    /// The verification page and one-time code, once the vendor has shown both.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<Prompt> {
        self.parser.process(bytes);
        let view = self.parser.screen().contents();
        let mut words = view.split_whitespace();
        let url = words.find_map(|word| {
            let parsed = url::Url::parse(word).ok()?;
            (parsed.scheme() == "https"
                && parsed.host_str() == Some("auth.openai.com")
                && parsed.path() == "/codex/device"
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.port().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none())
            .then_some(parsed)
        })?;
        let code = words.find(|word| device_code(word))?.to_owned();
        Some(Prompt { url, code })
    }
}

fn device_code(word: &str) -> bool {
    let Some((first, second)) = word.split_once('-') else {
        return false;
    };
    [first, second].iter().all(|part| {
        (4..=6).contains(&part.len())
            && part
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_prompt_yields_its_page_and_code_and_nothing_else_does() {
        let mut decoder = Decoder::new();
        assert!(
            decoder
                .feed(b"1. Open this link\r\n   \x1b[94mhttps://auth.openai.com/codex/device\x1b[0m\r\n")
                .is_none()
        );
        let prompt = decoder
            .feed(b"2. Enter this one-time code \x1b[90m(expires in 15 minutes)\x1b[0m\r\n   \x1b[94m0K88-JZ4OG\x1b[0m\r\n")
            .unwrap();
        assert_eq!(prompt.url.as_str(), "https://auth.openai.com/codex/device");
        assert_eq!(prompt.code, "0K88-JZ4OG");
        let mut elsewhere = Decoder::new();
        assert!(
            elsewhere
                .feed(b"https://auth.openai.com.evil/codex/device\r\nABCD-EFGH\r\n")
                .is_none()
        );
    }
}

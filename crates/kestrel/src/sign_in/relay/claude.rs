use anyhow::{Result, bail};

pub(super) struct Decoder {
    parser: vt100::Parser,
}

pub(super) struct Decoded {
    pub url: Option<String>,
    pub token: Option<String>,
    pub continue_prompt: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            parser: vt100::Parser::new(40, 240, 0),
        }
    }

    pub fn feed(&mut self, bytes: &[u8], answered: bool) -> Result<Decoded> {
        self.parser.process(bytes);
        let screen = self.parser.screen();
        let view = screen.contents();
        if answered && (view.contains("OAuth error:") || view.contains("Press Enter to retry")) {
            bail!("the vendor refused the authorization code");
        }
        let url = if !answered && view.contains("Paste code") {
            let rows: Vec<_> = screen.rows(0, 240).collect();
            rows.iter().enumerate().find_map(|(index, row)| {
                let start = row.find("https://")?;
                let mut url = row[start..].trim().to_owned();
                for row in &rows[index + 1..] {
                    let next = row.trim();
                    if next.is_empty() || next.contains(char::is_whitespace) {
                        break;
                    }
                    url.push_str(next);
                }
                let parsed = url::Url::parse(&url).ok()?;
                (parsed.scheme() == "https"
                    && parsed.host_str() == Some("claude.com")
                    && parsed.path() == "/cai/oauth/authorize"
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
                    && parsed.port().is_none()
                    && parsed.query().is_some()
                    && parsed.fragment().is_none())
                .then_some(url)
            })
        } else {
            None
        };
        Ok(Decoded {
            url,
            token: answered.then(|| token(&view, false)).flatten(),
            continue_prompt: view.contains("Press Enter") && !view.contains("retry"),
        })
    }

    pub fn finish(&self) -> Option<String> {
        token(&self.parser.screen().contents(), true)
    }
}

fn token(screen: &str, finished: bool) -> Option<String> {
    let start = screen.find("sk-ant-oat")?;
    let rest = &screen[start..];
    let length = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        .count();
    (length > "sk-ant-oat".len() && (length < rest.len() || finished))
        .then(|| rest[..length].to_owned())
}

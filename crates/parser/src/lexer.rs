use logos::Logos;
use std::ops::Range;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n]+")]
pub enum Token {
    #[token("@")]
    At,

    #[token("%")]
    Percent,

    #[token("~")]
    Tilde,

    #[token("!")]
    Bang,

    #[token("#")]
    Hash,

    #[token("&")]
    Amp,

    #[regex(r"\\[^\s.,]+")]
    Escaped,

    #[token("\\")]
    Backslash,

    #[regex(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")]
    IsoDate,

    #[regex(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([Zz]|[+-][0-9]{2}:[0-9]{2})")]
    Rfc3339,

    #[regex(r"[0-9]{1,2}:[0-9]{2}")]
    Time24,

    #[regex(r"[0-9]{1,2}(:[0-9]{2})?[aApP][mM]?")]
    Time12,

    #[regex(r"[0-9]{1,2}(st|nd|rd|th)")]
    OrdinalDay,

    #[regex(r"[0-9]+")]
    Number,

    #[regex(r"[A-Za-z_][A-Za-z0-9_\-'’]*")]
    Word,

    #[regex(r#""[^"]*""#)]
    Quoted,

    #[regex(r"[.,]")]
    Punct,
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Range<usize>,
    pub text: String,
}

pub fn lex(input: &str) -> Vec<SpannedToken> {
    let mut lexer = Token::lexer(input);
    let mut out = Vec::new();

    while let Some(result) = lexer.next() {
        let span = lexer.span();
        let text = input[span.clone()].to_string();
        let token = match result {
            Ok(tok) => tok,
            Err(_) => Token::Word,
        };
        out.push(SpannedToken { token, span, text });
    }

    out
}

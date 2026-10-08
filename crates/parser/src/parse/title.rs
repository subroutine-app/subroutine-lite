use crate::{
    ast::{HighlightKind, ParseDraft},
    lexer::{SpannedToken, Token},
};

pub(super) fn assemble_title(draft: &mut ParseDraft, tokens: &[SpannedToken], consumed: &[bool]) {
    let mut title = String::new();
    let mut previous_end = None;
    let template = draft.kind.is_template();
    for (idx, tok) in tokens.iter().enumerate() {
        if consumed[idx] {
            continue;
        }
        let text = match tok.token {
            Token::Word | Token::Number | Token::Quoted => strip_quotes(&tok.text),
            Token::Escaped => strip_escape(&tok.text),
            _ if template => tok.text.clone(),
            _ => continue,
        };
        if !title.is_empty() && (!template || previous_end != Some(tok.span.start)) {
            title.push(' ');
        }
        title.push_str(&text);
        previous_end = Some(tok.span.end);
        draft
            .highlights
            .push((tok.span.clone(), HighlightKind::Title));
    }
    draft.title = title.trim().to_string();
}

pub(super) fn is_escaped(tokens: &[SpannedToken], at: usize) -> bool {
    matches!(tokens.get(at).map(|t| &t.token), Some(Token::Escaped))
}

fn strip_quotes(text: &str) -> String {
    if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

fn strip_escape(text: &str) -> String {
    strip_quotes(text.strip_prefix('\\').unwrap_or(text))
}

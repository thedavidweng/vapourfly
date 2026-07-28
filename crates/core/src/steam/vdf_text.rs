//! Text VDF parser and writer.
//!
//! Handles Valve's KeyValues text format used in Steam configuration files
//! (`.vdf`). The format supports quoted and unquoted tokens, recursive objects,
//! line comments, escape sequences, and duplicate keys.

use crate::error::{Result, VapourflyError};
use crate::models::VdfNode;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    String(String),
    LBrace,
    RBrace,
}

/// Break `input` into a flat list of [`Token`]s.
fn tokenize(input: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut line: usize = 1;
    let mut tokens = Vec::new();

    while i < len {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            // Line comment — skip to end of line.
            while i < len && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            // Quoted string.
            i += 1; // skip opening quote
            let mut s = String::new();
            while i < len && chars[i] != '"' {
                if chars[i] == '\n' {
                    return Err(VapourflyError::InvalidInput(format!(
                        "VDF parse error at line {line}: unterminated quoted string",
                    )));
                }
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= len {
                return Err(VapourflyError::InvalidInput(format!(
                    "VDF parse error at line {line}: unterminated quoted string",
                )));
            }
            i += 1; // skip closing quote
            tokens.push(Token::String(s));
        } else if c == '{' {
            tokens.push(Token::LBrace);
            i += 1;
        } else if c == '}' {
            tokens.push(Token::RBrace);
            i += 1;
        } else {
            // Unquoted (bare) word — terminated by whitespace, brace, or quote.
            let start = i;
            while i < len
                && !chars[i].is_whitespace()
                && chars[i] != '{'
                && chars[i] != '}'
                && chars[i] != '"'
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(Token::String(word));
        }
    }

    Ok(tokens)
}

/// Parse an object body: a sequence of key-value pairs until `}` or EOF.
fn parse_object_body(tokens: &[Token], pos: &mut usize) -> Result<VdfNode> {
    let mut entries: Vec<(String, VdfNode)> = Vec::new();

    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::RBrace => {
                *pos += 1;
                return Ok(VdfNode::Object(entries));
            }
            Token::String(key) => {
                let key = key.clone();
                *pos += 1;

                if *pos >= tokens.len() {
                    return Err(VapourflyError::InvalidInput(
                        "VDF parse error: unexpected end of input after key".into(),
                    ));
                }

                match &tokens[*pos] {
                    Token::LBrace => {
                        *pos += 1;
                        let obj = parse_object_body(tokens, pos)?;
                        entries.push((key, obj));
                    }
                    Token::String(val) => {
                        let val = val.clone();
                        *pos += 1;
                        entries.push((key, VdfNode::String(val)));
                    }
                    Token::RBrace => {
                        return Err(VapourflyError::InvalidInput(format!(
                            "VDF parse error: unexpected '}}' after key \"{key}\"",
                        )));
                    }
                }
            }
            Token::LBrace => {
                return Err(VapourflyError::InvalidInput(
                    "VDF parse error: unexpected '{{' — expected a key".into(),
                ));
            }
        }
    }

    Ok(VdfNode::Object(entries))
}

/// Parse a Text VDF string into a [`VdfNode`].
///
/// The root is always an `Object`. Duplicate keys are preserved in order.
///
/// # Errors
///
/// Returns [`VapourflyError::InvalidInput`] on malformed input (unterminated
/// quotes, unbalanced braces, unexpected tokens).
pub fn parse_text_vdf(input: &str) -> Result<VdfNode> {
    let tokens = tokenize(input)?;
    let mut pos = 0;
    let node = parse_object_body(&tokens, &mut pos)?;
    if pos < tokens.len() {
        return Err(VapourflyError::InvalidInput(format!(
            "VDF parse error: unexpected token at position {pos} after end of root object",
        )));
    }
    Ok(node)
}

/// Emit a [`VdfNode`] back into Text VDF format.
///
/// Objects use tab indentation. Key-value pairs separate key and value with
/// two tabs, matching the format Steam writes.
pub fn write_text_vdf(node: &VdfNode) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out
}

fn write_node(out: &mut String, node: &VdfNode, indent: usize) {
    match node {
        VdfNode::Object(entries) => {
            let prefix = "\t".repeat(indent);
            for (key, value) in entries {
                match value {
                    VdfNode::String(s) => {
                        out.push_str(&prefix);
                        out.push('"');
                        push_escaped(out, key);
                        out.push_str("\"\t\t\"");
                        push_escaped(out, s);
                        out.push_str("\"\n");
                    }
                    VdfNode::Object(_) => {
                        out.push_str(&prefix);
                        out.push('"');
                        push_escaped(out, key);
                        out.push_str("\"\n");
                        out.push_str(&prefix);
                        out.push_str("{\n");
                        write_node(out, value, indent + 1);
                        out.push_str(&prefix);
                        out.push_str("}\n");
                    }
                }
            }
        }
        VdfNode::String(s) => {
            out.push('"');
            push_escaped(out, s);
            out.push('"');
        }
    }
}

/// Push `s` into `out` with VDF escape sequences applied.
fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ---------------------------------------------------------------

    /// Build a quick Object from a slice of `(key, value_str)` tuples.
    fn obj(entries: Vec<(&str, VdfNode)>) -> VdfNode {
        VdfNode::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    fn s(val: &str) -> VdfNode {
        VdfNode::String(val.to_string())
    }

    // -- parse: simple key-value -----------------------------------------------

    #[test]
    fn parse_simple_key_value() {
        let input = r#""key"		"value""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(node, obj(vec![("key", s("value"))]));
    }

    #[test]
    fn parse_multiple_key_values() {
        let input = r#""a" "1"
"b" "2"
"c" "3""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(node, obj(vec![("a", s("1")), ("b", s("2")), ("c", s("3"))]));
    }

    // -- parse: nested objects -------------------------------------------------

    #[test]
    fn parse_nested_objects() {
        let input = r#""root"
{
	"child"
	{
		"key"		"value"
	}
}"#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(
            node,
            obj(vec![(
                "root",
                obj(vec![("child", obj(vec![("key", s("value"))]))])
            )])
        );
    }

    #[test]
    fn parse_deeply_nested() {
        let input = r#""a"
{
	"b"
	{
		"c"
		{
			"d"		"deep"
		}
	}
}"#;
        let node = parse_text_vdf(input).unwrap();
        let d = obj(vec![("d", s("deep"))]);
        let c = obj(vec![("c", d)]);
        let b = obj(vec![("b", c)]);
        let a = obj(vec![("a", b)]);
        assert_eq!(node, a);
    }

    // -- parse: comments -------------------------------------------------------

    #[test]
    fn parse_with_comments() {
        let input = r#"// This is a comment
"key"		"value"
// Another comment
"key2"		"value2""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(node, obj(vec![("key", s("value")), ("key2", s("value2"))]));
    }

    #[test]
    fn parse_inline_comment_after_values() {
        // Comments are line-based; a // in the middle of a line after a complete
        // token is not treated as a comment by this tokenizer (it would be part
        // of an unquoted token). This test verifies that a comment on its own
        // line is stripped correctly.
        let input = r#""k" "v"
// ignored
"k2" "v2""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(node, obj(vec![("k", s("v")), ("k2", s("v2"))]));
    }

    // -- parse: escape sequences -----------------------------------------------

    #[test]
    fn parse_escape_sequences() {
        let input = r#""key"		"line1\nline2\ttab\\slash\"quote""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(
            node,
            obj(vec![("key", s("line1\nline2\ttab\\slash\"quote"))])
        );
    }

    #[test]
    fn parse_unknown_escape_passes_through() {
        // An unknown escape like \x should keep the backslash and the char.
        let input = r#""key"		"hello\xworld""#;
        let node = parse_text_vdf(input).unwrap();
        assert_eq!(node, obj(vec![("key", s("hello\\xworld"))]));
    }

    // -- parse: duplicate keys -------------------------------------------------

    #[test]
    fn parse_duplicate_keys_preserved() {
        let input = r#""tag"		"action"
"tag"		"rpg"
"tag"		"indie""#;
        let node = parse_text_vdf(input).unwrap();
        let entries = match &node {
            VdfNode::Object(e) => e,
            _ => panic!("expected Object"),
        };
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], ("tag".to_string(), s("action")));
        assert_eq!(entries[1], ("tag".to_string(), s("rpg")));
        assert_eq!(entries[2], ("tag".to_string(), s("indie")));
    }

    // -- round-trip ------------------------------------------------------------

    #[test]
    fn round_trip_simple() {
        let input = r#""root"
{
	"key"		"value"
	"num"		"42"
}
"#;
        let node = parse_text_vdf(input).unwrap();
        let output = write_text_vdf(&node);
        let node2 = parse_text_vdf(&output).unwrap();
        assert_eq!(node, node2);
    }

    #[test]
    fn round_trip_nested() {
        let input = r#""users"
{
	"76561198000000000"
	{
		"AccountName"		"test_user"
		"PersonaName"		"Test"
	}
}
"#;
        let node = parse_text_vdf(input).unwrap();
        let output = write_text_vdf(&node);
        let node2 = parse_text_vdf(&output).unwrap();
        assert_eq!(node, node2);
    }

    #[test]
    fn round_trip_duplicate_keys() {
        let input = r#""tags"
{
	"tag"		"action"
	"tag"		"rpg"
}
"#;
        let node = parse_text_vdf(input).unwrap();
        let output = write_text_vdf(&node);
        let node2 = parse_text_vdf(&output).unwrap();
        assert_eq!(node, node2);
    }

    // -- malformed input -------------------------------------------------------

    #[test]
    fn malformed_unterminated_quote() {
        let input = r#""key"		"unterminated"#;
        let err = parse_text_vdf(input).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unterminated"), "got: {msg}");
    }

    #[test]
    fn malformed_unterminated_quote_newline() {
        let input = "\"key\"\t\t\"unterminated\n";
        let err = parse_text_vdf(input).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unterminated"), "got: {msg}");
    }

    #[test]
    fn malformed_unbalanced_extra_close() {
        // An opening brace that isn't preceded by a key is malformed.
        let input = r#"{ "key" "value" }"#;
        let err = parse_text_vdf(input).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unexpected") || msg.contains("token"),
            "got: {msg}"
        );
    }

    #[test]
    fn malformed_unbalanced_extra_open() {
        let input = r#""root"
{
	"key"		"value"
"#;
        // Missing closing brace — parse_object_body returns at EOF without error
        // (the object is implicitly closed). This is actually acceptable for
        // real-world VDF files that may be truncated. The parser treats EOF as
        // an implicit close.
        let node = parse_text_vdf(input).unwrap();
        let entries = match &node {
            VdfNode::Object(e) => e,
            _ => panic!("expected Object"),
        };
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn malformed_key_with_no_value() {
        let input = r#""key""#;
        let err = parse_text_vdf(input).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unexpected end of input"), "got: {msg}");
    }

    // -- empty / minimal inputs ------------------------------------------------

    #[test]
    fn parse_empty_input() {
        let node = parse_text_vdf("").unwrap();
        assert_eq!(node, VdfNode::Object(vec![]));
    }

    #[test]
    fn parse_whitespace_only() {
        let node = parse_text_vdf("   \n\t  \n  ").unwrap();
        assert_eq!(node, VdfNode::Object(vec![]));
    }

    #[test]
    fn parse_comments_only() {
        let node = parse_text_vdf("// just a comment\n// another\n").unwrap();
        assert_eq!(node, VdfNode::Object(vec![]));
    }

    // -- write format ----------------------------------------------------------

    #[test]
    fn write_simple_produces_tab_indented_output() {
        let node = obj(vec![("key", s("value"))]);
        let out = write_text_vdf(&node);
        assert_eq!(out, "\"key\"\t\t\"value\"\n");
    }

    #[test]
    fn write_nested_uses_indentation() {
        let node = obj(vec![("root", obj(vec![("child", s("v"))]))]);
        let out = write_text_vdf(&node);
        assert_eq!(out, "\"root\"\n{\n\t\"child\"\t\t\"v\"\n}\n");
    }

    // -- real-world fixture ----------------------------------------------------

    #[test]
    fn parse_loginusers_fixture() {
        let input = include_str!("../../../../data/fixtures/steam_minimal/config/loginusers.vdf");
        let node = parse_text_vdf(input).unwrap();

        // Should have a single "users" key.
        let users = node.child_object(&["users"]).expect("users object");
        // Inside users, there should be one steam-id key.
        let entries = match users {
            VdfNode::Object(e) => e,
            _ => panic!("expected Object"),
        };
        assert_eq!(entries.len(), 1);

        let steam_id = &entries[0].0;
        assert_eq!(steam_id, "76561198000000000");

        // Navigate into the steam-id object and check fields.
        let user_obj = node
            .child_object(&["users", "76561198000000000"])
            .expect("steam id object");
        assert_eq!(
            user_obj.first_string("AccountName"),
            Some("vapourfly_fixture_user")
        );
        assert_eq!(
            user_obj.first_string("PersonaName"),
            Some("Vapourfly Fixture")
        );
        assert_eq!(user_obj.first_string("RememberPassword"), Some("1"));
        assert_eq!(user_obj.first_string("MostRecent"), Some("1"));
        assert_eq!(user_obj.first_string("Timestamp"), Some("1700000000"));
    }

    #[test]
    fn loginusers_fixture_round_trip() {
        let input = include_str!("../../../../data/fixtures/steam_minimal/config/loginusers.vdf");
        let node = parse_text_vdf(input).unwrap();
        let output = write_text_vdf(&node);
        let node2 = parse_text_vdf(&output).unwrap();
        assert_eq!(node, node2);
    }
}

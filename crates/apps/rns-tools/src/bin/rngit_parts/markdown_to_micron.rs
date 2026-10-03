//! Bounded Markdown conversion for rngit's converted-download endpoint.
//!
//! Keep this independent of the Python runtime. Unsupported Markdown remains
//! literal text rather than being silently discarded from a downloaded file.

pub(super) fn convert(source: &str, url_scope: &str) -> Option<Vec<u8>> {
    let mut lines = Vec::new();
    let mut code = Vec::new();
    let mut in_code = false;
    for line in source.split('\n') {
        if line.trim_start().starts_with("```") {
            if in_code {
                flush_code(&mut code, &mut lines);
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code.push(line);
            continue;
        }
        lines.push(convert_line(line, url_scope));
    }
    if in_code {
        flush_code(&mut code, &mut lines);
    }
    let converted = lines.join("\n");
    let converted = converted.trim_end();
    (!converted.is_empty()).then(|| converted.as_bytes().to_vec())
}

fn flush_code(code: &mut Vec<&str>, lines: &mut Vec<String>) {
    if code.is_empty() {
        return;
    }
    lines.push("`BT282828`Fddd".to_string());
    lines.push("`=".to_string());
    lines.push(code.join("\n").replace('`', "\\`"));
    lines.push("`=".to_string());
    lines.push("`f`b".to_string());
    code.clear();
}

fn convert_line(line: &str, url_scope: &str) -> String {
    let escaped = line.replace('\\', "\\\\");
    let trimmed = escaped.trim_start();
    if let Some((marks, content)) = trimmed.split_once(' ') {
        if !marks.is_empty() && marks.len() <= 6 && marks.bytes().all(|byte| byte == b'#') {
            return format!("{}{}", ">".repeat(marks.len()), convert_inline(content, url_scope));
        }
    }
    if matches!(trimmed, "---" | "----" | "***" | "___" | "===") {
        return "-".to_string();
    }
    if let Some((indent, content)) = list_item(&escaped) {
        return format!("{indent} • {}", convert_inline(content, url_scope));
    }
    if let Some(quote) = escaped.strip_prefix('>') {
        return format!(" │ {}", convert_inline(quote.trim_start(), url_scope));
    }
    if escaped.starts_with('<') || (escaped.starts_with('-') && !escaped.starts_with("- ")) {
        return format!("\\{}", convert_inline(&escaped, url_scope));
    }
    convert_inline(&escaped, url_scope)
}

fn list_item(line: &str) -> Option<(&str, &str)> {
    let indent_len = line.bytes().take_while(|byte| *byte == b' ' || *byte == b'\t').count();
    let rest = &line[indent_len..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '-' | '*' | '+') || !rest[marker.len_utf8()..].starts_with(' ') {
        return None;
    }
    Some((&line[..indent_len], rest[marker.len_utf8()..].trim_start()))
}

fn convert_inline(input: &str, url_scope: &str) -> String {
    let mut result = String::new();
    let mut rest = input;
    while !rest.is_empty() {
        if let Some((label, target, consumed)) = markdown_link(rest) {
            let target = if target.contains(":/") {
                target.to_string()
            } else if let Some((path, anchor)) = target.split_once('#') {
                format!("{url_scope}{path}|anchor={anchor}")
            } else {
                format!("{url_scope}{target}")
            };
            result.push_str(&format!("`_`!`[{}{}{}]`!`_", label.replace('`', ""), "`", target));
            rest = &rest[consumed..];
            continue;
        }
        if let Some((content, consumed)) = delimited(rest, "`", false) {
            result.push_str("`BT383838`Fddd");
            result.push_str(&content.replace('`', "\\`"));
            result.push_str("`f`b");
            rest = &rest[consumed..];
            continue;
        }
        if let Some((content, consumed)) =
            delimited(rest, "**", true).or_else(|| delimited(rest, "__", true))
        {
            result.push_str("`!");
            result.push_str(content);
            result.push_str("`!");
            rest = &rest[consumed..];
            continue;
        }
        if let Some((content, consumed)) =
            delimited(rest, "*", true).or_else(|| delimited(rest, "_", true))
        {
            result.push_str("`*");
            result.push_str(content);
            result.push_str("`*");
            rest = &rest[consumed..];
            continue;
        }
        let character = rest.chars().next().expect("nonempty remainder has a character");
        result.push(character);
        rest = &rest[character.len_utf8()..];
    }
    result
}

fn delimited<'a>(
    input: &'a str,
    delimiter: &str,
    require_content: bool,
) -> Option<(&'a str, usize)> {
    let remainder = input.strip_prefix(delimiter)?;
    let end = remainder.find(delimiter)?;
    if require_content && end == 0 {
        return None;
    }
    let consumed = delimiter.len() + end + delimiter.len();
    Some((&remainder[..end], consumed))
}

fn markdown_link(input: &str) -> Option<(&str, &str, usize)> {
    let remainder = input.strip_prefix('[')?;
    let close = remainder.find("](")?;
    let target_start = close + 2;
    let target_end = remainder[target_start..].find(')')? + target_start;
    if close == 0 || target_start == target_end {
        return None;
    }
    Some((&remainder[..close], &remainder[target_start..target_end], target_end + 2))
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn common_markdown_matches_pinned_python_1_5_5_converter() {
        let source =
            "# Heading\n\nHello **bold** and *italic*.\n\n- item\n\n[relative](docs/intro.md)\n";
        let actual = convert(source, ":/page/blob.mu`g=group|r=repo|ref=HEAD|path=")
            .expect("converted Markdown");
        assert_eq!(
            String::from_utf8(actual).expect("UTF-8"),
            ">Heading\n\nHello `!bold`! and `*italic`*.\n\n • item\n\n`_`!`[relative`:/page/blob.mu`g=group|r=repo|ref=HEAD|path=docs/intro.md]`!`_"
        );
    }
}

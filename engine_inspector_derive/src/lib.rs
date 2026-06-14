use proc_macro::TokenStream;

#[proc_macro_derive(Inspector)]
pub fn derive_inspector(input: TokenStream) -> TokenStream {
    match expand(input.to_string()) {
        Ok(output) => match output.parse() {
            Ok(tokens) => tokens,
            Err(error) => compile_error(error.to_string()).parse().unwrap(),
        },
        Err(error) => compile_error(error).parse().unwrap(),
    }
}

fn expand(input: String) -> Result<String, String> {
    let struct_name = parse_struct_name(&input)?;
    let body = parse_struct_body(&input)?;
    let mut calls = Vec::new();

    for field in split_top_level(&body, ',') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        if field.contains("inspector") && field.contains("skip") {
            continue;
        }

        let (name, ty) = parse_field(field)?;
        let compact_ty = ty.split_whitespace().collect::<String>();
        let call = if compact_ty == "f32" {
            format!(r#"visitor.field_f32("{name}", &mut self.{name});"#)
        } else if compact_ty == "i32" {
            format!(r#"visitor.field_i32("{name}", &mut self.{name});"#)
        } else if compact_ty == "u32" {
            format!(r#"visitor.field_u32("{name}", &mut self.{name});"#)
        } else if compact_ty == "bool" {
            format!(r#"visitor.field_bool("{name}", &mut self.{name});"#)
        } else if compact_ty.starts_with("[f32;") && compact_ty.ends_with(']') {
            format!(r#"visitor.field_f32_array("{name}", &mut self.{name});"#)
        } else {
            return Err(format!(
                "Inspector derive does not support field `{name}` with type `{ty}`"
            ));
        };
        calls.push(call);
    }

    Ok(format!(
        r#"
impl crate::renderer::Inspector for {struct_name} {{
    fn inspect(&mut self, visitor: &mut dyn crate::renderer::InspectorVisitor) {{
        {}
    }}
}}
"#,
        calls.join("\n")
    ))
}

fn parse_struct_name(input: &str) -> Result<String, String> {
    let mut tokens = input.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "struct" {
            return tokens
                .next()
                .map(|name| {
                    name.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .to_string()
                })
                .filter(|name| !name.is_empty())
                .ok_or_else(|| "Inspector derive expected a named struct".to_string());
        }
    }
    Err("Inspector derive only supports structs".to_string())
}

fn parse_struct_body(input: &str) -> Result<String, String> {
    let start = input
        .find('{')
        .ok_or_else(|| "Inspector derive only supports structs with named fields".to_string())?;
    let end = input
        .rfind('}')
        .ok_or_else(|| "Inspector derive could not find the end of the struct body".to_string())?;
    Ok(input[start + 1..end].to_string())
}

fn parse_field(field: &str) -> Result<(String, String), String> {
    let colon = find_top_level(field, ':')
        .ok_or_else(|| format!("Inspector derive expected `name: type`, got `{field}`"))?;
    let raw_name = field[..colon].trim();
    let name = raw_name
        .split_whitespace()
        .last()
        .ok_or_else(|| format!("Inspector derive could not parse field name in `{field}`"))?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_string();

    if name.is_empty() {
        return Err(format!(
            "Inspector derive could not parse field name in `{field}`"
        ));
    }

    Ok((name, field[colon + 1..].trim().to_string()))
}

fn split_top_level(input: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;

    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                parts.push(input[start..index].to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(input[start..].to_string());
    parts
}

fn find_top_level(input: &str, target: char) -> Option<usize> {
    let mut depth = 0_i32;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => depth -= 1,
            _ if ch == target && depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn compile_error(error: String) -> String {
    format!("compile_error!({error:?});")
}

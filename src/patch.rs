use anyhow::{Result, bail};
use std::collections::BTreeMap;

/// Keep the original patch text: quoting, comments, aliases and list order matter.
pub fn bodies(content: &str) -> Result<BTreeMap<String, String>> {
    let mut active = false;
    let mut name = None;
    let mut result: BTreeMap<String, String> = BTreeMap::new();
    for line in content.lines() {
        if !active {
            if line.trim_end() == "patch_files:" {
                active = true;
            }
            continue;
        }
        if !line.is_empty() && !line.starts_with([' ', '\t', '#']) {
            break;
        }
        if line.starts_with("  ")
            && !line.starts_with("   ")
            && !line.trim_start().starts_with('#')
            && line.trim_end().ends_with(':')
        {
            let key = line.trim().trim_end_matches(':');
            let key: String = yaml_serde::from_str(key)?;
            result.insert(key.clone(), String::new());
            name = Some(key);
        } else if let Some(name) = &name {
            let body = result.get_mut(name).unwrap();
            body.push_str(line.strip_prefix("  ").unwrap_or(line));
            body.push('\n');
        }
    }
    Ok(result)
}

pub fn apply(existing: &str, body: &str, identity: &str) -> Result<String> {
    if identity.contains(['\r', '\n']) {
        bail!("配方标识不能包含换行");
    }
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let marker = format!("# Rx: {identity} {{");
    let mut lines = Vec::new();
    let mut removing = false;
    for line in existing.lines() {
        if line == marker {
            removing = true;
            continue;
        }
        if removing {
            if line == "# }" {
                removing = false;
            }
            continue;
        }
        lines.push(line.to_string());
    }
    if removing {
        bail!("已有配方补丁标记未闭合: {identity}");
    }
    let header = lines.iter().position(|line| line.trim_end() == "__patch:");
    let header = if let Some(header) = header {
        header
    } else {
        lines.push("__patch:".into());
        lines.len() - 1
    };
    let end = (header + 1..lines.len())
        .find(|&i| !lines[i].is_empty() && !lines[i].starts_with([' ', '\t', '#']))
        .unwrap_or(lines.len());
    let mut block = vec![marker];
    block.extend(body.lines().map(str::to_string));
    block.push("# }".into());
    lines.splice(end..end, block);
    Ok(lines.join(newline) + newline)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_whole_recipe_and_preserves_other_text() {
        let original =
            "# user comment\nschema: \"quoted\"\n__patch:\n  user/key: true\nother: value\n";
        let first = apply(original, "  x: 1\n  stale: true\n", "demo::").unwrap();
        let other = apply(&first, "  other: true\n", "other::").unwrap();
        let updated = apply(&other, "  x: 2\n", "demo::").unwrap();
        assert!(!updated.contains("stale"));
        assert!(updated.contains("# user comment\nschema: \"quoted\""));
        assert!(updated.contains("# Rx: other:: {"));
        assert_eq!(apply(&updated, "  x: 2\n", "demo::").unwrap(), updated);
    }
    #[test]
    fn sequence_and_raw_comments() {
        let content = "patch_files:\n  a.yaml:\n    # retained\n    - foo.yaml:/patch\n    - bar.yaml:/patch\n";
        let bodies = bodies(content).unwrap();
        let patched = apply("", &bodies["a.yaml"], "demo::").unwrap();
        let value: yaml_serde::Value = yaml_serde::from_str(&patched).unwrap();
        assert!(value["__patch"].is_sequence());
        assert!(patched.contains("# retained"));
    }
}

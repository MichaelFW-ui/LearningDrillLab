use serde::Serialize;
use std::fs;
use std::path::PathBuf;

const MAX_SKILL_BYTES: u64 = 64 * 1024;

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub source: String,
}

fn custom_path(name: &str) -> Option<PathBuf> {
    if !matches!(name, "curriculum" | "experiment-verification") {
        return None;
    }
    crate::app::storage_path()?
        .parent()
        .map(|directory| directory.join("skills").join(name).join("SKILL.md"))
}

fn read_custom(name: &str) -> Option<String> {
    let path = custom_path(name)?;
    if fs::metadata(&path).ok()?.len() > MAX_SKILL_BYTES {
        return None;
    }
    let body = fs::read_to_string(path).ok()?;
    if !body.starts_with("---\n") || !body.contains(&format!("\nname: {name}\n")) {
        return None;
    }
    Some(body)
}

pub fn load(name: &str) -> String {
    read_custom(name).unwrap_or_else(|| match name {
        "curriculum" => include_str!("../skills/curriculum/SKILL.md").to_string(),
        "experiment-verification" => {
            include_str!("../skills/experiment-verification/SKILL.md").to_string()
        }
        _ => String::new(),
    })
}

pub fn list() -> Vec<SkillInfo> {
    [
        ("curriculum", "选择练习动作、难度与结束时机"),
        ("experiment-verification", "设计并核对沙箱实验"),
    ]
    .into_iter()
    .map(|(name, description)| SkillInfo {
        name,
        description,
        source: if read_custom(name).is_some() {
            "本地自定义".to_string()
        } else {
            "应用内置".to_string()
        },
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registered_skill_paths_are_allowed() {
        assert!(custom_path("../curriculum").is_none());
        assert!(load("curriculum").contains("name: curriculum"));
    }
}

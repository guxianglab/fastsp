use std::collections::HashSet;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::storage::PromptProfile;

pub const ENABLE_POLISH_SKILL_ID: &str = "enable_polish";
pub const DISABLE_POLISH_SKILL_ID: &str = "disable_polish";
pub const SWITCH_POLISH_SCENE_SKILL_ID: &str = "switch_polish_scene";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillConfig {
    pub id: String,
    pub name: String,
    pub keywords: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMatch {
    pub skill_id: String,
    pub keyword: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinSkillId {
    ComposeEmail,
    OpenCalculator,
    OpenBrowser,
    OpenNotepad,
    OpenExplorer,
    Screenshot,
    EnablePolish,
    DisablePolish,
    SwitchPolishScene,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneResolveResult {
    Unique { profile_id: String, profile_name: String },
    None,
    Ambiguous(Vec<String>),
}

impl BuiltinSkillId {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "compose_email" => Some(Self::ComposeEmail),
            "open_calculator" => Some(Self::OpenCalculator),
            "open_browser" => Some(Self::OpenBrowser),
            "open_notepad" => Some(Self::OpenNotepad),
            "open_explorer" => Some(Self::OpenExplorer),
            "screenshot" => Some(Self::Screenshot),
            ENABLE_POLISH_SKILL_ID => Some(Self::EnablePolish),
            DISABLE_POLISH_SKILL_ID => Some(Self::DisablePolish),
            SWITCH_POLISH_SCENE_SKILL_ID => Some(Self::SwitchPolishScene),
            _ => None,
        }
    }

    fn is_config_control(&self) -> bool {
        matches!(
            self,
            Self::EnablePolish | Self::DisablePolish | Self::SwitchPolishScene
        )
    }
}

pub fn is_config_skill(skill_id: &str) -> bool {
    BuiltinSkillId::from_str(skill_id)
        .map(|skill| skill.is_config_control())
        .unwrap_or(false)
}

pub fn get_default_skills() -> Vec<SkillConfig> {
    vec![
        SkillConfig {
            id: "compose_email".to_string(),
            name: "写邮件".to_string(),
            keywords: "写邮件,发邮件,新邮件,发送邮件".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "open_calculator".to_string(),
            name: "计算器".to_string(),
            keywords: "计算器,打开计算器".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "open_browser".to_string(),
            name: "浏览器".to_string(),
            keywords: "打开浏览器,上网,浏览器".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "open_notepad".to_string(),
            name: "记事本".to_string(),
            keywords: "记事本,打开记事本".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "open_explorer".to_string(),
            name: "文件管理器".to_string(),
            keywords: "文件管理器,资源管理器,打开文件夹".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "screenshot".to_string(),
            name: "截图".to_string(),
            keywords: "截图,截屏,屏幕截图".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: ENABLE_POLISH_SKILL_ID.to_string(),
            name: "启用润色".to_string(),
            keywords: "启用润色,打开润色,开启润色".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: DISABLE_POLISH_SKILL_ID.to_string(),
            name: "关闭润色".to_string(),
            keywords: "关闭润色,停用润色,禁用润色".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
            name: "切换润色场景".to_string(),
            keywords: "切换到,切到,使用".to_string(),
            enabled: true,
        },
    ]
}

pub fn merge_with_default_skills(existing_skills: Vec<SkillConfig>) -> (Vec<SkillConfig>, bool) {
    let mut merged = existing_skills;
    let existing_ids: HashSet<String> = merged.iter().map(|skill| skill.id.clone()).collect();
    let mut changed = false;

    for skill in get_default_skills() {
        if !existing_ids.contains(&skill.id) {
            merged.push(skill);
            changed = true;
        }
    }

    (merged, changed)
}

pub fn match_skills(text: &str, skills: &[SkillConfig]) -> Vec<SkillMatch> {
    let text_lower = text.to_lowercase();
    let mut matches: Vec<(usize, usize, SkillMatch)> = Vec::new();

    for (skill_index, skill) in skills.iter().enumerate() {
        if !skill.enabled {
            continue;
        }

        let mut best_match: Option<SkillMatch> = None;

        for raw_keyword in skill.keywords.split(',') {
            let keyword = raw_keyword.trim();
            if keyword.is_empty() {
                continue;
            }

            let keyword_lower = keyword.to_lowercase();
            if let Some(pos) = text_lower.find(&keyword_lower) {
                let candidate = SkillMatch {
                    skill_id: skill.id.clone(),
                    keyword: keyword.to_string(),
                    start: pos,
                    end: pos + keyword_lower.len(),
                };

                let should_replace = best_match
                    .as_ref()
                    .map(|existing| candidate.start < existing.start)
                    .unwrap_or(true);

                if should_replace {
                    best_match = Some(candidate);
                }
            }
        }

        if let Some(skill_match) = best_match {
            println!(
                "[SKILL] Matched skill '{}' with keyword '{}'",
                skill_match.skill_id, skill_match.keyword
            );
            matches.push((skill_match.start, skill_index, skill_match));
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    matches.into_iter().map(|(_, _, skill_match)| skill_match).collect()
}

pub fn extract_scene_query(
    transcript: &str,
    current: &SkillMatch,
    next: Option<&SkillMatch>,
) -> String {
    let end = next.map(|skill_match| skill_match.start).unwrap_or(transcript.len());
    if current.end >= end
        || current.end > transcript.len()
        || !transcript.is_char_boundary(current.end)
        || !transcript.is_char_boundary(end)
    {
        return String::new();
    }

    transcript[current.end..end]
        .trim_matches(is_scene_boundary_char)
        .to_string()
}

pub fn resolve_scene(profiles: &[PromptProfile], raw_query: &str) -> SceneResolveResult {
    let normalized_query = normalize_scene_token(raw_query);
    if normalized_query.is_empty() {
        return SceneResolveResult::None;
    }

    let alias_matches = collect_scene_matches(profiles, &normalized_query, true);
    if !alias_matches.is_empty() {
        return collapse_scene_matches(alias_matches);
    }

    collapse_scene_matches(collect_scene_matches(profiles, &normalized_query, false))
}

fn collect_scene_matches(
    profiles: &[PromptProfile],
    normalized_query: &str,
    use_aliases: bool,
) -> Vec<(String, String)> {
    let mut matches = Vec::new();
    let mut seen_ids = HashSet::new();

    for profile in profiles {
        let candidates: Vec<&str> = if use_aliases {
            profile.voice_aliases.iter().map(String::as_str).collect()
        } else {
            vec![profile.name.as_str()]
        };

        if candidates
            .into_iter()
            .map(normalize_scene_token)
            .any(|candidate| !candidate.is_empty() && candidate == normalized_query)
            && seen_ids.insert(profile.id.clone())
        {
            matches.push((profile.id.clone(), profile.name.clone()));
        }
    }

    matches
}

fn collapse_scene_matches(matches: Vec<(String, String)>) -> SceneResolveResult {
    match matches.len() {
        0 => SceneResolveResult::None,
        1 => {
            let (profile_id, profile_name) = matches.into_iter().next().unwrap();
            SceneResolveResult::Unique {
                profile_id,
                profile_name,
            }
        }
        _ => {
            let mut names: Vec<String> = matches.into_iter().map(|(_, name)| name).collect();
            names.sort();
            names.dedup();
            SceneResolveResult::Ambiguous(names)
        }
    }
}

pub fn normalize_scene_token(value: &str) -> String {
    let mut normalized: String = value
        .chars()
        .filter(|ch| !is_ignored_scene_char(*ch))
        .flat_map(|ch| ch.to_lowercase())
        .collect();

    loop {
        if let Some(stripped) = normalized.strip_suffix("场景") {
            normalized = stripped.to_string();
            continue;
        }
        if let Some(stripped) = normalized.strip_suffix("模式") {
            normalized = stripped.to_string();
            continue;
        }
        break;
    }

    normalized
}

fn is_ignored_scene_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | ':'
                | ';'
                | '!'
                | '?'
                | '/'
                | '\\'
                | '|'
                | '-'
                | '_'
                | '，'
                | '。'
                | '：'
                | '；'
                | '！'
                | '？'
                | '、'
                | '《'
                | '》'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '"'
                | '\''
                | '('
                | ')'
                | '（'
                | '）'
        )
}

fn is_scene_boundary_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | ':'
                | ';'
                | '!'
                | '?'
                | '，'
                | '。'
                | '：'
                | '；'
                | '！'
                | '？'
                | '、'
        )
}

pub fn execute_skill(skill_id: &str) -> Result<(), String> {
    match BuiltinSkillId::from_str(skill_id) {
        Some(BuiltinSkillId::ComposeEmail) => {
            println!("[SKILL] Executing: compose_email");
            Command::new("cmd")
                .args(["/C", "start", "mailto:"])
                .spawn()
                .map_err(|e| format!("Failed to open email client: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenCalculator) => {
            println!("[SKILL] Executing: open_calculator");
            Command::new("calc")
                .spawn()
                .map_err(|e| format!("Failed to open calculator: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenBrowser) => {
            println!("[SKILL] Executing: open_browser");
            Command::new("cmd")
                .args(["/C", "start", "https://"])
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenNotepad) => {
            println!("[SKILL] Executing: open_notepad");
            Command::new("notepad")
                .spawn()
                .map_err(|e| format!("Failed to open notepad: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenExplorer) => {
            println!("[SKILL] Executing: open_explorer");
            Command::new("explorer")
                .spawn()
                .map_err(|e| format!("Failed to open explorer: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::Screenshot) => {
            println!("[SKILL] Executing: screenshot (Win+Shift+S)");
            Command::new("powershell")
                .args([
                    "-Command",
                    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('+#{s}')",
                ])
                .spawn()
                .map_err(|e| format!("Failed to trigger screenshot: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::EnablePolish)
        | Some(BuiltinSkillId::DisablePolish)
        | Some(BuiltinSkillId::SwitchPolishScene) => Err(format!(
            "Config skill '{}' must be handled by the transcription pipeline",
            skill_id
        )),
        None => Err(format!("Unknown skill: {}", skill_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched_ids(text: &str, skills: &[SkillConfig]) -> Vec<String> {
        match_skills(text, skills)
            .into_iter()
            .map(|skill_match| skill_match.skill_id)
            .collect()
    }

    #[test]
    fn test_match_skills_single() {
        let skills = vec![
            SkillConfig {
                id: "compose_email".to_string(),
                name: "Compose Email".to_string(),
                keywords: "compose email,write email".to_string(),
                enabled: true,
            },
            SkillConfig {
                id: "open_calculator".to_string(),
                name: "Open Calculator".to_string(),
                keywords: "calculator,open calculator".to_string(),
                enabled: true,
            },
        ];

        assert_eq!(
            matched_ids("compose email", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            matched_ids("please write email to the team", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            matched_ids("open calculator", &skills),
            vec!["open_calculator".to_string()]
        );
        assert!(matched_ids("the weather is nice today", &skills).is_empty());
    }

    #[test]
    fn test_match_skills_multiple_in_transcript_order() {
        let skills = vec![
            SkillConfig {
                id: "open_calculator".to_string(),
                name: "Open Calculator".to_string(),
                keywords: "calculator,open calculator".to_string(),
                enabled: true,
            },
            SkillConfig {
                id: "open_browser".to_string(),
                name: "Open Browser".to_string(),
                keywords: "browser,open browser".to_string(),
                enabled: true,
            },
        ];

        assert_eq!(
            matched_ids("browser and calculator", &skills),
            vec!["open_browser".to_string(), "open_calculator".to_string()]
        );
    }

    #[test]
    fn test_disabled_skill() {
        let mut skills = vec![SkillConfig {
            id: "compose_email".to_string(),
            name: "Compose Email".to_string(),
            keywords: "compose email,write email".to_string(),
            enabled: true,
        }];
        skills[0].enabled = false;

        assert!(matched_ids("compose email", &skills).is_empty());
    }

    #[test]
    fn test_match_skills_deduplicates_per_skill() {
        let skills = vec![SkillConfig {
            id: "open_browser".to_string(),
            name: "Open Browser".to_string(),
            keywords: "browser,open browser".to_string(),
            enabled: true,
        }];

        assert_eq!(
            matched_ids("please open browser in the browser", &skills),
            vec!["open_browser".to_string()]
        );
    }

    #[test]
    fn test_extract_scene_query_stops_before_next_skill() {
        let matches = vec![
            SkillMatch {
                skill_id: ENABLE_POLISH_SKILL_ID.to_string(),
                keyword: "启用润色".to_string(),
                start: 0,
                end: "启用润色".len(),
            },
            SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: "启用润色".len(),
                end: "启用润色切换到".len(),
            },
        ];

        assert_eq!(
            extract_scene_query("启用润色切换到邮件", &matches[1], None),
            "邮件".to_string()
        );
    }

    #[test]
    fn test_resolve_scene_prefers_aliases() {
        let profiles = vec![
            PromptProfile {
                id: "email".to_string(),
                name: "邮件写作".to_string(),
                voice_aliases: vec!["邮件".to_string()],
                ..PromptProfile::new_default()
            },
            PromptProfile {
                id: "notes".to_string(),
                name: "邮件".to_string(),
                voice_aliases: vec![],
                ..PromptProfile::new_default()
            },
        ];

        assert_eq!(
            resolve_scene(&profiles, "邮件场景"),
            SceneResolveResult::Unique {
                profile_id: "email".to_string(),
                profile_name: "邮件写作".to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_scene_falls_back_to_name() {
        let profiles = vec![PromptProfile {
            id: "email".to_string(),
            name: "邮件".to_string(),
            voice_aliases: vec![],
            ..PromptProfile::new_default()
        }];

        assert_eq!(
            resolve_scene(&profiles, "邮件模式"),
            SceneResolveResult::Unique {
                profile_id: "email".to_string(),
                profile_name: "邮件".to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_scene_detects_alias_conflicts() {
        let profiles = vec![
            PromptProfile {
                id: "email".to_string(),
                name: "邮件".to_string(),
                voice_aliases: vec!["客服".to_string()],
                ..PromptProfile::new_default()
            },
            PromptProfile {
                id: "support".to_string(),
                name: "客服".to_string(),
                voice_aliases: vec!["客服".to_string()],
                ..PromptProfile::new_default()
            },
        ];

        assert_eq!(
            resolve_scene(&profiles, "客服"),
            SceneResolveResult::Ambiguous(vec!["客服".to_string(), "邮件".to_string()])
        );
    }
}

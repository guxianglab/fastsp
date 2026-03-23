use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillConfig {
    pub id: String,
    pub name: String,
    pub keywords: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuiltinSkillId {
    ComposeEmail,
    OpenCalculator,
    OpenBrowser,
    OpenNotepad,
    OpenExplorer,
    Screenshot,
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
            _ => None,
        }
    }
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
    ]
}

pub fn match_skills(text: &str, skills: &[SkillConfig]) -> Vec<String> {
    let text_lower = text.to_lowercase();
    let mut matches: Vec<(usize, usize, String)> = Vec::new();

    for (skill_index, skill) in skills.iter().enumerate() {
        if !skill.enabled {
            continue;
        }

        let mut first_match_pos: Option<usize> = None;
        let mut matched_keyword: Option<String> = None;

        for keyword in skill.keywords.split(',') {
            let keyword = keyword.trim().to_lowercase();
            if keyword.is_empty() {
                continue;
            }

            if let Some(pos) = text_lower.find(&keyword) {
                let should_replace = match first_match_pos {
                    Some(existing_pos) => pos < existing_pos,
                    None => true,
                };

                if should_replace {
                    first_match_pos = Some(pos);
                    matched_keyword = Some(keyword);
                }
            }
        }

        if let (Some(pos), Some(keyword)) = (first_match_pos, matched_keyword) {
            println!(
                "[SKILL] Matched skill '{}' with keyword '{}'",
                skill.id, keyword
            );
            matches.push((pos, skill_index, skill.id.clone()));
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    matches.into_iter().map(|(_, _, skill_id)| skill_id).collect()
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
        None => Err(format!("Unknown skill: {}", skill_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            match_skills("compose email", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            match_skills("please write email to the team", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            match_skills("open calculator", &skills),
            vec!["open_calculator".to_string()]
        );
        assert!(match_skills("the weather is nice today", &skills).is_empty());
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
            match_skills("browser and calculator", &skills),
            vec![
                "open_browser".to_string(),
                "open_calculator".to_string()
            ]
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

        assert!(match_skills("compose email", &skills).is_empty());
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
            match_skills("please open browser in the browser", &skills),
            vec!["open_browser".to_string()]
        );
    }
}

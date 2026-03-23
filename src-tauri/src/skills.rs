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

pub fn match_skill(text: &str, skills: &[SkillConfig]) -> Option<String> {
    let text_lower = text.to_lowercase();

    for skill in skills {
        if !skill.enabled {
            continue;
        }

        for keyword in skill.keywords.split(',') {
            let keyword = keyword.trim().to_lowercase();
            if !keyword.is_empty() && text_lower.contains(&keyword) {
                println!(
                    "[SKILL] Matched skill '{}' with keyword '{}'",
                    skill.id, keyword
                );
                return Some(skill.id.clone());
            }
        }
    }

    None
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
    fn test_match_skill() {
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
            match_skill("compose email", &skills),
            Some("compose_email".to_string())
        );
        assert_eq!(
            match_skill("please write email to the team", &skills),
            Some("compose_email".to_string())
        );
        assert_eq!(
            match_skill("open calculator", &skills),
            Some("open_calculator".to_string())
        );
        assert_eq!(match_skill("the weather is nice today", &skills), None);
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

        assert_eq!(match_skill("compose email", &skills), None);
    }
}

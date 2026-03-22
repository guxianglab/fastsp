use serde::{Deserialize, Serialize};
use std::process::Command;

/// 技能配置
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillConfig {
    /// 技能唯一标识
    pub id: String,
    /// 技能显示名称
    pub name: String,
    /// 触发关键词列表（逗号分隔存储，匹配时拆分）
    pub keywords: String,
    /// 是否启用
    pub enabled: bool,
}

/// 预置技能ID
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
            "compose_email" => Some(BuiltinSkillId::ComposeEmail),
            "open_calculator" => Some(BuiltinSkillId::OpenCalculator),
            "open_browser" => Some(BuiltinSkillId::OpenBrowser),
            "open_notepad" => Some(BuiltinSkillId::OpenNotepad),
            "open_explorer" => Some(BuiltinSkillId::OpenExplorer),
            "screenshot" => Some(BuiltinSkillId::Screenshot),
            _ => None,
        }
    }
}

/// 获取默认技能列表
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
            keywords: "计算器,算一下,打开计算器".to_string(),
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
            keywords: "记事本,写笔记,打开记事本".to_string(),
            enabled: true,
        },
        SkillConfig {
            id: "open_explorer".to_string(),
            name: "文件管理器".to_string(),
            keywords: "文件管理器,我的电脑,资源管理器,打开文件夹".to_string(),
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

/// 模糊匹配：检查识别文本是否包含任一关键词
/// 返回匹配到的技能ID
pub fn match_skill(text: &str, skills: &[SkillConfig]) -> Option<String> {
    let text_lower = text.to_lowercase();

    for skill in skills {
        if !skill.enabled {
            continue;
        }

        // 拆分关键词并检查是否匹配
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

/// 执行技能
pub fn execute_skill(skill_id: &str) -> Result<(), String> {
    let builtin = BuiltinSkillId::from_str(skill_id);

    match builtin {
        Some(BuiltinSkillId::ComposeEmail) => {
            // 打开默认邮件客户端新建邮件
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
            // 使用 start 命令打开默认浏览器
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
            // 使用 PowerShell 发送 Win+Shift+S 快捷键
            Command::new("powershell")
                .args([
                    "-Command",
                    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('+#{s}')"
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
        let skills = get_default_skills();

        // 精确匹配
        assert_eq!(
            match_skill("写邮件", &skills),
            Some("compose_email".to_string())
        );

        // 包含匹配
        assert_eq!(
            match_skill("帮我写封邮件", &skills),
            Some("compose_email".to_string())
        );
        assert_eq!(
            match_skill("打开计算器", &skills),
            Some("open_calculator".to_string())
        );

        // 无匹配
        assert_eq!(match_skill("今天天气不错", &skills), None);
    }

    #[test]
    fn test_disabled_skill() {
        let mut skills = get_default_skills();
        skills[0].enabled = false; // 禁用写邮件

        assert_eq!(match_skill("写邮件", &skills), None);
    }
}

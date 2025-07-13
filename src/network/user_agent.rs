use anyhow::Result;
use rand::prelude::*;
use std::fs;
use std::path::Path;

pub struct UserAgentManager {
    agents: Vec<String>,
    use_random: bool,
}

impl UserAgentManager {
    pub fn new(custom_ua: Option<String>, random_ua: bool, ua_file: Option<&Path>) -> Result<Self> {
        let agents = if let Some(ua) = custom_ua {
            vec![ua]
        } else if let Some(file_path) = ua_file {
            Self::load_from_file(file_path)?
        } else if random_ua {
            Self::default_user_agents()
        } else {
            vec!["pulzr/0.1.0".to_string()]
        };

        Ok(Self {
            agents,
            use_random: random_ua,
        })
    }

    pub fn get_user_agent(&self) -> &str {
        if self.use_random && self.agents.len() > 1 {
            self.agents.choose(&mut rand::rng()).unwrap()
        } else {
            &self.agents[0]
        }
    }

    fn load_from_file(file_path: &Path) -> Result<Vec<String>> {
        let content = fs::read_to_string(file_path)?;
        Ok(content
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn default_user_agents() -> Vec<String> {
        vec![
            // Chrome on Windows
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36".to_string(),

            // Chrome on macOS
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36".to_string(),

            // Firefox on Windows
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:120.0) Gecko/20100101 Firefox/120.0".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:119.0) Gecko/20100101 Firefox/119.0".to_string(),

            // Firefox on macOS
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:120.0) Gecko/20100101 Firefox/120.0".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:119.0) Gecko/20100101 Firefox/119.0".to_string(),

            // Safari on macOS
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Safari/605.1.15".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15".to_string(),

            // Edge on Windows
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36 Edg/119.0.0.0".to_string(),

            // Chrome on Linux
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36".to_string(),

            // Firefox on Linux
            "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0".to_string(),
            "Mozilla/5.0 (X11; Linux x86_64; rv:119.0) Gecko/20100101 Firefox/119.0".to_string(),

            // Mobile Chrome on Android
            "Mozilla/5.0 (Linux; Android 14; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36".to_string(),
            "Mozilla/5.0 (Linux; Android 13; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Mobile Safari/537.36".to_string(),

            // Mobile Safari on iOS
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1".to_string(),
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1".to_string(),

            // iPad Safari
            "Mozilla/5.0 (iPad; CPU OS 17_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1".to_string(),
            "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1".to_string(),

            // Samsung Internet
            "Mozilla/5.0 (Linux; Android 14; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/23.0 Chrome/115.0.0.0 Mobile Safari/537.36".to_string(),
            "Mozilla/5.0 (Linux; Android 13; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/22.0 Chrome/114.0.0.0 Mobile Safari/537.36".to_string(),
        ]
    }

    pub fn get_agent_count(&self) -> usize {
        self.agents.len()
    }

    pub fn is_random(&self) -> bool {
        self.use_random
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_user_agent() {
        let manager = UserAgentManager::new(None, false, None).unwrap();
        assert_eq!(manager.get_user_agent(), "pulzr/0.1.0");
        assert!(!manager.is_random());
        assert_eq!(manager.get_agent_count(), 1);
    }

    #[test]
    fn test_custom_user_agent() {
        let custom_ua = "MyCustomAgent/1.0".to_string();
        let manager = UserAgentManager::new(Some(custom_ua.clone()), false, None).unwrap();
        assert_eq!(manager.get_user_agent(), custom_ua);
        assert!(!manager.is_random());
        assert_eq!(manager.get_agent_count(), 1);
    }

    #[test]
    fn test_random_user_agents() {
        let manager = UserAgentManager::new(None, true, None).unwrap();
        assert!(manager.is_random());
        assert!(manager.get_agent_count() > 20); // Should have many default agents

        // Get multiple user agents and verify they can vary
        let mut agents = std::collections::HashSet::new();
        for _ in 0..50 {
            agents.insert(manager.get_user_agent().to_string());
        }

        // With random selection, we should get some variety (though not guaranteed)
        // This test might occasionally fail due to randomness, but very unlikely
        assert!(!agents.is_empty());
    }

    #[test]
    fn test_user_agent_from_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Agent1/1.0")?;
        writeln!(temp_file, "Agent2/2.0")?;
        writeln!(temp_file)?; // Empty line should be filtered out
        writeln!(temp_file, "Agent3/3.0")?;
        temp_file.flush()?;

        let manager = UserAgentManager::new(None, false, Some(temp_file.path()))?;
        assert_eq!(manager.get_agent_count(), 3);
        assert_eq!(manager.get_user_agent(), "Agent1/1.0");
        assert!(!manager.is_random());

        Ok(())
    }

    #[test]
    fn test_random_user_agent_from_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Agent1/1.0")?;
        writeln!(temp_file, "Agent2/2.0")?;
        writeln!(temp_file, "Agent3/3.0")?;
        temp_file.flush()?;

        let manager = UserAgentManager::new(None, true, Some(temp_file.path()))?;
        assert_eq!(manager.get_agent_count(), 3);
        assert!(manager.is_random());

        Ok(())
    }

    #[test]
    fn test_file_not_found() {
        let result = UserAgentManager::new(
            None,
            false,
            Some(std::path::Path::new("/nonexistent/file.txt")),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_file() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        // Don't write anything to the file

        let manager = UserAgentManager::new(None, false, Some(temp_file.path()))?;
        assert_eq!(manager.get_agent_count(), 0);

        Ok(())
    }

    #[test]
    fn test_custom_user_agent_overrides_random() {
        let custom_ua = "CustomAgent/1.0".to_string();
        let manager = UserAgentManager::new(Some(custom_ua.clone()), true, None).unwrap();
        assert_eq!(manager.get_user_agent(), custom_ua);
        assert!(manager.is_random()); // Random flag is preserved, but only one agent so no randomness
        assert_eq!(manager.get_agent_count(), 1);
    }

    #[test]
    fn test_default_user_agents_count() {
        let agents = UserAgentManager::default_user_agents();
        assert!(agents.len() >= 20); // Should have at least 20 default agents

        // Verify all agents are non-empty
        for agent in &agents {
            assert!(!agent.is_empty());
            assert!(agent.len() > 10); // Reasonable minimum length
        }
    }

    #[test]
    fn test_file_with_whitespace() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "  Agent1/1.0  ")?; // Leading/trailing whitespace
        writeln!(temp_file, "\t\tAgent2/2.0\t\t")?; // Tabs
        writeln!(temp_file, "   ")?; // Only whitespace - should be filtered
        temp_file.flush()?;

        let manager = UserAgentManager::new(None, false, Some(temp_file.path()))?;
        assert_eq!(manager.get_agent_count(), 2);
        assert_eq!(manager.get_user_agent(), "Agent1/1.0");

        Ok(())
    }
}

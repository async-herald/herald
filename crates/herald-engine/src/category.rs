use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Security,
    CodeSmell,
    Dependencies,
    Architecture,
    Ai,
    Git,
    Performance,
    Pattern,
    Reliability,
    Testing,
}

impl Category {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Security     => "security",
            Self::CodeSmell    => "code-smell",
            Self::Dependencies => "dependencies",
            Self::Architecture => "architecture",
            Self::Ai           => "ai-pattern",
            Self::Git          => "git",
            Self::Performance  => "performance",
            Self::Pattern      => "pro-pattern",
            Self::Reliability  => "reliability",
            Self::Testing      => "testing",
        }
    }
}

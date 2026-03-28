use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    #[default]
    Public,
    Cookie,
    Header,
    Intercept,
    Ui,
}

impl Strategy {
    pub fn requires_browser(&self) -> bool {
        !matches!(self, Self::Public)
    }
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Cookie => write!(f, "cookie"),
            Self::Header => write!(f, "header"),
            Self::Intercept => write!(f, "intercept"),
            Self::Ui => write!(f, "ui"),
        }
    }
}

use std::fmt;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorCategory {
    Unavailable,
    Permission,
    Timeout,
    Parse,
    Io,
    Backend,
}

impl ProviderErrorCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Permission => "permission",
            Self::Timeout => "timeout",
            Self::Parse => "parse",
            Self::Io => "io",
            Self::Backend => "backend",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub category: ProviderErrorCategory,
    pub message: String,
}

impl ProviderError {
    pub fn new(category: ProviderErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCategory::Unavailable, message)
    }

    pub fn permission(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCategory::Permission, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCategory::Timeout, message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCategory::Parse, message)
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorCategory::Backend, message)
    }

    pub fn from_io(operation: &str, error: &io::Error) -> Self {
        let category = match error.kind() {
            io::ErrorKind::NotFound => ProviderErrorCategory::Unavailable,
            io::ErrorKind::PermissionDenied => ProviderErrorCategory::Permission,
            io::ErrorKind::TimedOut => ProviderErrorCategory::Timeout,
            _ => ProviderErrorCategory::Io,
        };
        Self::new(category, format!("{operation}: {error}"))
    }

    pub fn diagnostic(&self) -> String {
        format!("{}: {}", self.category.as_str(), self.message)
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.category.as_str(), self.message)
    }
}

impl std::error::Error for ProviderError {}

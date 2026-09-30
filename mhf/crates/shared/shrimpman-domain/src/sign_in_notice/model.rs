use crate::TimeRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInNotice {
    pub id: u32,
    pub content: String,
    pub period: TimeRange,
    pub priority: i32,
}

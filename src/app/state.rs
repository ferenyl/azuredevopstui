#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    MyPrs,
    MyWorkItems,
    OtherPrs,
    ReadyWorkItems,
    Detail,
}

pub const LEFT_PANELS: [Panel; 4] = [
    Panel::MyPrs,
    Panel::MyWorkItems,
    Panel::OtherPrs,
    Panel::ReadyWorkItems,
];

impl Panel {
    pub fn title(self) -> &'static str {
        match self {
            Self::MyPrs => "My PRs",
            Self::MyWorkItems => "My work items",
            Self::OtherPrs => "Others' PRs",
            Self::ReadyWorkItems => "Ready (sprint)",
            Self::Detail => "Details",
        }
    }
}

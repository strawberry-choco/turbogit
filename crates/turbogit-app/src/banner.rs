#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerSeverity {
    Info,
    Warning,
    Error,
    Success,
}

pub struct BannerAction {
    pub label: String,
    pub on_click: Box<dyn FnOnce(&mut crate::state::AppState)>,
}

impl BannerAction {
    pub fn new(
        label: impl Into<String>,
        on_click: impl FnOnce(&mut crate::state::AppState) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            on_click: Box::new(on_click),
        }
    }
}

pub struct Banner {
    pub severity: BannerSeverity,
    pub message: String,
    pub actions: Vec<BannerAction>,
}

impl Banner {
    pub fn new(severity: BannerSeverity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            actions: Vec::new(),
        }
    }

    pub fn action(mut self, a: BannerAction) -> Self {
        self.actions.push(a);
        self
    }
}

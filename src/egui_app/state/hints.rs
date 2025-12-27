/// UI state for the hint-of-the-day popup.
#[derive(Clone, Debug)]
pub struct HintOfDayState {
    pub open: bool,
    pub show_on_startup: bool,
    pub title: String,
    pub body: String,
}

impl Default for HintOfDayState {
    fn default() -> Self {
        Self {
            open: false,
            show_on_startup: true,
            title: String::new(),
            body: String::new(),
        }
    }
}

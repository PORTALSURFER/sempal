use super::FolderBrowserState;

const SELECTION_FLASH_FRAMES: u8 = 12;

impl FolderBrowserState {
    pub(in crate::native_app) fn flash_marked_item(&mut self, item_id: String) {
        self.selection_flash_item_id = Some(item_id);
        self.selection_flash_frames = SELECTION_FLASH_FRAMES;
    }

    pub(in crate::native_app) fn clear_marked_item_flash(&mut self, item_id: &str) {
        if self.selection_flash_item_id.as_deref() == Some(item_id) {
            self.selection_flash_item_id = None;
            self.selection_flash_frames = 0;
        }
    }

    pub(in crate::native_app) fn selection_flash_active(&self) -> bool {
        self.selection_flash_frames > 0
    }

    pub(in crate::native_app) fn selection_flash_frames(&self) -> u8 {
        self.selection_flash_frames
    }

    pub(in crate::native_app) fn advance_selection_flash_frame(&mut self) {
        if self.selection_flash_frames == 0 {
            return;
        }
        self.selection_flash_frames = self.selection_flash_frames.saturating_sub(1);
        if self.selection_flash_frames == 0 {
            self.selection_flash_item_id = None;
        }
    }

    pub(super) fn marked_item_flash_active(&self, item_id: &str) -> bool {
        self.selection_flash_active() && self.selection_flash_item_id.as_deref() == Some(item_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marking_another_item_restarts_and_retargets_the_flash() {
        let mut browser = FolderBrowserState::load_default();

        browser.flash_marked_item(String::from("first"));
        browser.advance_selection_flash_frame();
        assert!(browser.marked_item_flash_active("first"));

        browser.flash_marked_item(String::from("second"));
        assert!(!browser.marked_item_flash_active("first"));
        assert!(browser.marked_item_flash_active("second"));
        assert_eq!(browser.selection_flash_frames(), SELECTION_FLASH_FRAMES);
    }

    #[test]
    fn marked_item_flash_expires_after_its_frame_budget() {
        let mut browser = FolderBrowserState::load_default();
        browser.flash_marked_item(String::from("sample"));

        for _ in 0..SELECTION_FLASH_FRAMES {
            browser.advance_selection_flash_frame();
        }

        assert!(!browser.selection_flash_active());
        assert!(!browser.marked_item_flash_active("sample"));
    }

    #[test]
    fn unmarking_the_flashing_item_clears_positive_feedback() {
        let mut browser = FolderBrowserState::load_default();
        browser.flash_marked_item(String::from("sample"));

        browser.clear_marked_item_flash("sample");

        assert!(!browser.selection_flash_active());
        assert!(!browser.marked_item_flash_active("sample"));
    }
}

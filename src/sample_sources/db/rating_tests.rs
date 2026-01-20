#[cfg(test)]
mod tests {
    use crate::sample_sources::Rating;

    #[test]
    fn test_rating_clamping() {
        assert_eq!(Rating::new(5).val(), 3);
        assert_eq!(Rating::new(-5).val(), -3);
        assert_eq!(Rating::new(0).val(), 0);
    }

    #[test]
    fn test_persisted_rating_mapping() {
        assert_eq!(Rating::from_i64(1).val(), 1);
        assert_eq!(Rating::from_i64(2).val(), 2);
        assert_eq!(Rating::from_i64(3).val(), 3);
        assert_eq!(Rating::from_i64(-3).val(), -3);
        assert_eq!(Rating::from_i64(0).val(), 0);
    }

    #[test]
    fn test_classification_helpers() {
        assert!(Rating::TRASH_3.is_trash());
        assert!(Rating::TRASH_1.is_trash());
        assert!(!Rating::NEUTRAL.is_trash());
        
        assert!(Rating::KEEP_3.is_keep());
        assert!(Rating::KEEP_1.is_keep());
        assert!(!Rating::NEUTRAL.is_keep());
        
        assert!(Rating::NEUTRAL.is_neutral());
        assert!(!Rating::TRASH_1.is_neutral());
    }
}

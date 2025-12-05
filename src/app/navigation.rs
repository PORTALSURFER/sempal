/// Compute a new selection index clamped to the bounds of the wav list.
pub(super) fn compute_target_index(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match current {
        Some(index) => {
            let max_index = len.saturating_sub(1) as isize;
            Some((index as isize + delta).clamp(0, max_index) as usize)
        }
        None => {
            if delta >= 0 {
                Some(0)
            } else {
                Some(len.saturating_sub(1))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_target_index_clamps_bounds() {
        assert_eq!(compute_target_index(Some(0), 3, -1), Some(0));
        assert_eq!(compute_target_index(Some(2), 3, 1), Some(2));
        assert_eq!(compute_target_index(Some(1), 3, -1), Some(0));
        assert_eq!(compute_target_index(Some(1), 3, 1), Some(2));
    }

    #[test]
    fn compute_target_index_initializes_when_none() {
        assert_eq!(compute_target_index(None, 3, 1), Some(0));
        assert_eq!(compute_target_index(None, 3, -1), Some(2));
    }

    #[test]
    fn compute_target_index_handles_empty() {
        assert_eq!(compute_target_index(None, 0, 1), None);
        assert_eq!(compute_target_index(Some(0), 0, 1), None);
    }
}

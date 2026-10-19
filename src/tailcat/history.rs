/// Peer-option flag read by the recent-peer list. Must match
/// `kSealDeskHideRecent` in the Flutter client.
pub(crate) const HIDE_RECENT_OPTION: &str = "sealdesk-hide-recent";

/// Every recent peer id, including ones the Windows UI has not loaded yet.
/// The first paint only receives `PeerConfig::BATCH_LOADING_COUNT` peers.
pub(crate) fn recent_ids_for_history_clear(
    all_ids: impl IntoIterator<Item = String>,
) -> Vec<String> {
    all_ids.into_iter().filter(|id| !id.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use hbb_common::config::PeerConfig;

    use super::recent_ids_for_history_clear;

    #[test]
    fn clear_marks_peers_beyond_the_first_windows_batch() {
        let total = PeerConfig::BATCH_LOADING_COUNT + 3;
        let mut ids: Vec<String> = (0..total).map(|i| format!("peer-{i}")).collect();
        ids.insert(1, String::new());
        let marked = recent_ids_for_history_clear(ids.clone());
        assert_eq!(marked.len(), total);
        assert!(marked.len() > PeerConfig::BATCH_LOADING_COUNT);
        assert_eq!(
            marked[PeerConfig::BATCH_LOADING_COUNT],
            ids[PeerConfig::BATCH_LOADING_COUNT + 1]
        );
        assert!(!marked.iter().any(String::is_empty));
    }
}

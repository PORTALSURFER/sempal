Goal
- Ensure sample source scanning only runs when a source is newly added or when the user manually requests it via a right-click context menu on the selected source item in the source list.

Proposed solutions
- Decouple source selection from automatic scans, keeping initial scans only when a new source is added while using existing database contents for navigation.
- Add a right-click context menu on source list rows that exposes manual "Rescan / Find changes" alongside existing actions, targeting the currently selected source.
- Route the new manual action through the existing scan pipeline (ScanTracker, start_scan_for) with forced scans and clear status updates.

Step-by-step plan
1. [x] Map current scan triggers and source list interactions (DropHandler selection/add flows, SourcePanel menu) to spot where automatic scans need to be removed or gated.
2. [x] Update selection and initial load behaviour so existing sources only load cached entries without triggering scans, while preserving the first-time scan when adding a new source.
3. [x] Extend SourcePanel to show a right-click context menu on selected rows with a "Rescan / Find changes" action (and keep/remove other actions as appropriate) without disrupting tap/selection behaviour.
4. [x] Wire the new menu action into DropHandler to invoke a manual rescan on the targeted source using start_scan_for(force=true) and align status text/ScanTracker rules.
5. [~] Verify flows manually or via tests: add source triggers one scan; selecting sources does not rescan; right-click rescan works and the wav list updates without regressions.

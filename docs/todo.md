- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

..

  - TODO 3: Break up src/sample_sources/scanner/scan.rs into phases (walk, diff, db_sync) and introduce a ScanContext
    struct for the shared state (existing entries, stats, mode). This will simplify scan() and make it easier to unit
    test individual steps.
  - TODO 4: Extract I/O vs. signal processing in src/analysis/audio/decode.rs. Create decode_io.rs (file probing/
    decoding) and analysis_prep.rs (mono prep, trimming, normalization). This isolates failure modes and makes
    testing the pure DSP steps simpler.
  - TODO 5: Reduce test duplication in src/egui_app/controller/tests/waveform.rs and src/egui_app/controller/tests/
    browser_actions.rs by creating focused fixtures in src/egui_app/controller/test_support.rs (e.g.,
    “prepare_with_source_and_wav_entries” and “load_waveform_selection”). This will shorten tests, reduce setup
    variance, and make changes safer.

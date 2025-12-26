pub(crate) fn ensure_downloads() -> Result<(), String> {
    sempal::model_setup::ensure_panns_burnpack(
        sempal::model_setup::PannsSetupOptions::default(),
    )?;
    Ok(())
}

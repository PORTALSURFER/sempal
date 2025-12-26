use crate::ui;

pub(crate) fn ensure_downloads(sender: &ui::InstallerSender) -> Result<(), String> {
    send_log(sender, "Using bundled ML model")?;
    Ok(())
}

fn send_log(sender: &ui::InstallerSender, message: &str) -> Result<(), String> {
    sender
        .send(ui::InstallerEvent::Log(message.to_string()))
        .map_err(|err| format!("Failed to send log update: {err}"))
}
